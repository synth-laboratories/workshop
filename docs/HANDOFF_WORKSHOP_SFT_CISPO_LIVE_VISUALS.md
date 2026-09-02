# Workshop handoff: live SFT + CISPO visuals

## 2026-09-02 closeout — Desktop click-through is proven

The implementation and Workshop proof are complete on these revisions:

- Workshop: `226224b2` (`bdda44db` terminal watcher drain, `226224b2` aggregate paired-heldout presentation)
- Optimizers: `0b5f46c` (explicit run-scoped idempotency), on top of `c464392` (streamed paired selection/heldout evaluation)

The NanoClassify reference split is now the product contract:

- 9,233 training examples, 400 stratified selection examples, 400 heldout examples, 77 labels.
- Selection evaluation streams during training and chooses a checkpoint.
- The heldout split is not inspected “during” training. It runs once after selection, comparing the unchanged base and selected checkpoint on the same 400 examples.
- Accuracy, macro-F1, exact paired uplift, bootstrap 95% CI, McNemar exact p-value, minimum practical uplift, and a claim-ready verdict are producer facts. The visual never infers an uplift claim from training completion or checkpoint readiness.

Full-size unpaid fixture runs were launched by clicking the Workshop cards and observed live:

| Algorithm | Workshop run | Terminal cursor | Scale proven |
|---|---|---:|---|
| SFT | `sft_banking77_nanoclassify_reference_5f81bc86` | 2,917 | 100 steps, batch 64, checkpoints 25/50/75/100, five 400-example selection passes, paired 400-example closeout |
| CISPO | `cispo_hosted_379e0d3d5fbe` | 3,718 | 50 updates, 150 groups, group size 64 (9,600 sampled rollouts), five checkpoints/evals, paired 400-example closeout |

Both visuals crossed the former 500-event cutoff while still updating. SFT finished with 100 curve points and four ready checkpoints. CISPO finished with 50 curve points, five ready checkpoints, `150/150` rollout groups, clip `0…5`, and an honest uniform-reward/no-learning-signal result.

The final shared heldout card shows base score, selected score, paired N, uplift, paired 95% CI, verdict, and whether an uplift claim is established. On the fixture both scores are zero, CI is zero, and the verdict is `inconclusive`; this proves plumbing, not model quality.

Two races found during the proof are fixed:

1. The hosted mirror drains full 500-event producer pages even when the producer already reports terminal.
2. The Workshop watcher requires a follow-up empty page after a terminal status, so it cannot settle between reading page N and the sidecar appending page N+1.

Distinct Workshop launches now honor their supplied run ID as the public-service idempotency scope. Retrying the same run still deduplicates; launching the same recipe again creates a new run instead of colliding with a configuration-only key.

### Remaining paid proof boundary

No authorized project-local `TINKER_API_KEY` is present, so no paid model training was run. Do not use Keychain. Start the existing public services without fixture mode once a project-local `.env` or Workshop proxy credential is available; the combined experiment remains capped at $50.

NanoClassify's real reference evidence remains the expectation check: SFT moved the fixed 400-example evaluation from 35.25% to 51.25% (+16 points); the cited CISPO v5 continuation added only one example (+0.25 points), which is not a material uplift. A new paid run must report its own paired heldout result and cannot reuse those numbers as its result.

The remainder of this document preserves the original diagnostic handoff for provenance; sections that call the Desktop path unproven are historical.

**Owner:** Workshop Desktop (`workshop-readmodel-cua`).  
**Do not redo:** public `optimizers` control planes, CISPO CLI, fixture executors, `cispo.request.v1` submit shape. Those are done and proven.  
**Your job:** click a hosted Banking77 recipe in Desktop, keep the live visual open, and watch `metric_points` grow the way GEPA live grows `candidates` / `evaluations`.

Related producer brief (already executed):
[`HANDOFF_SFT_CISPO_LIVE_GEPA_PARITY.md`](HANDOFF_SFT_CISPO_LIVE_GEPA_PARITY.md).

---

## Identity (read this twice)

| What the user sees | Recipe / placement | Provider |
|---|---|---|
| Hosted Banking77 SFT | `sft.banking77.nemotron-lightning.tinker.v1` | Tinker |
| Hosted Banking77 CISPO | `cispo.banking77.tinker.v1` | Tinker |
| Hosted CISPO alias | `cispo.hosted.tinker.v1` | Tinker |
| This Mac MLX SFT | `sft.qwen35-2b.mlx.v1` | MLX |
| This Mac MLX CISPO | `cispo.mlx.v1` | MLX |

**Slime (Modal) is not this stack.** Do not put `slime` in recipe ids, button labels, or visual titles.  
**`implementation=slime-reference` / `implementation_version=cispo.slime.v1` is the CISPO math pin** (clip bounds 0 / 5). Public `validate_cispo_request` requires those strings. Do not rename them. Legacy recipe ids `cispo.slime.hosted.v1` and `cispo.banking77.slime.tinker.v1` still dispatch as aliases only.

| Surface | `algorithm_id` | Not |
|---|---|---|
| Standalone SFT | `sft` | not `go-ex` |
| True CISPO | `cispo` | not generic IS |
| GoEx SFT lane | `go-ex` | not this work |

Craftax is rust GameBench gold only (`env:craftax_gold`). Do not invent fixture worlds. Public `optimizers` only — no `optimizers-beta`, no `:8787`.

Paid CISPO stays fail-closed until `TINKER_CISPO_VALIDATION_RECEIPT` points at:

`optimizers/docs/receipts/tinker-gpt-oss-20b-banking77-canary-cispo/cispo.slime.v1.receipt.json`

(`schema_version=tinker.capability_validation.v1`, `capability=cispo.slime.v1`, `validated=true`, `paid_update=true`).  
`SYNTH_OPTIMIZERS_CISPO_HOSTED_ADMITTED` still does **nothing**.

---

## What is already true (do not rebuild)

### Public services (`optimizers`)

Fixture Banking77 jobs were run on this machine against:

- SFT `127.0.0.1:8878` — run `sft_banking77_stream_qa`
- CISPO `127.0.0.1:8880` — run `cispo_banking77_stream_qa`

Both completed. `GET /v1/runs/{id}/optimizer-events?after_sequence=` returned `optimizer_event_page.v1` pages that **grew while status was `running`**. SSE `/optimizer-events/stream` mirrored the same events. Disconnecting SSE does not stop the job.

SFT kinds that matter for curves: `sft.step.metrics` (twice).  
CISPO kinds that matter: `cispo.clip.identity`, then `cispo.update.completed`.

Producer helpers:

```bash
# from /Users/joshuapurtell/GitHub/optimizers
export SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN=local-qa-token
export SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN=local-qa-token
export SYNTH_OPTIMIZERS_SFT_FIXTURE=1
export SYNTH_OPTIMIZERS_CISPO_FIXTURE=1

uv run python -m synth_optimizers.cli sft service --db .live-qa/sft.sqlite --bind 127.0.0.1:8878
uv run python -m synth_optimizers.cispo_cli service --fixture --db .live-qa/cispo.sqlite --bind 127.0.0.1:8880
```

Probe that already passed: `optimizers/.live-qa/stream_probe.py`. Re-run it if you doubt the producer. Do not treat a green probe as a green **visual**.

### Desktop launch / mapping (code, unproven live)

- Hosted CISPO `start_hosted` no longer binds a container. It POSTs `{ "config_json": <cispo.request.v1> }`.
- `drive_hosted_cispo_job` skips the tunneled evaluation plan when `config_json` is `cispo.request.v1`.
- UI: Optimizers Launch tab has This Mac + hosted Tinker buttons (`start-sft-hosted`, `start-cispo-hosted`). Hosted Tinker does not send `containerId` and does not require an SFT warm-start.
- Visuals: `optimizer.sft.live.v1`, `optimizer.cispo.live.v1`. Renderer is **projection-first** (`PROJECTION_FIRST_ALGORITHMS` includes `sft` and `cispo`). Charts hydrate from collection `metric_points`, never from the raw journal.
- Mapping prefers `training_adapter::adapt_source_fact`, with a sidecar fallback.

---

## Architecture Guy owns

GEPA live and hosted Tinker live share the **visual contract** (collections + `optimizer.run.updated`) and **not** the ingest path.

```
GEPA
  recipes::start
    → Desktop-owned `gepa service` (ephemeral)
    → GET {gepa}/runs/{id}/optimizer-events?after_sequence=
    → ingest_event_page → local journal
    → visual optimizer.gepa.live.v1 reads candidates | evaluations | proposer_calls

Hosted SFT / CISPO
  OptimizersPage.startBoundedRecipe(recipeId, openVisual: true)
    → OptimizerService.start_recipe
    → hosted_sft::start / cispo::start_hosted
    → sidecar_training::create_and_watch
         1. require Optimizers sidecar ready (GEPA plugin)     ← still required
         2. POST sidecar /v1/training/jobs  { placement, config }
         3. sidecar TrainingRuntime::drive_hosted_*_job
              polls PUBLIC :8878 / :8880 optimizer-events
              copies events into sidecar job.events
         4. spawn_watch_worker → watch_job
              polls SIDECAR /v1/training/jobs/{id}/events?after=
              append_mapped_event → kernel collections
         5. bus optimizer.run.updated
    → visual optimizer.sft.live.v1 / optimizer.cispo.live.v1
         subscribeToRun (wakeup + 750ms poll)
         refetch metric_points | candidates | evaluations (| rollouts)
```

Desktop **does not** consume Python SSE. SSE is a CLI mirror. Do not wire the renderer to `/optimizer-events/stream`.

Desktop **does not** spawn `:8878` / `:8880`. Do not fold SFT/CISPO into `gepa service`. The GEPA sidecar is only the training job driver + watch.

`hosted_sft::spawn_hosted_worker` (`ingest_event_page`) is `#[allow(dead_code)]`. Live path is `create_and_watch`. Do not run both.

---

## The likely bug (start here)

Two-hop copy already understands public `sequence_number`. The sidecar event list endpoint does **not**.

`drive_hosted_sft_job` / `drive_hosted_cispo_job` (`sidecar_training.rs`):

- Read `sequence_number` or `sequence` from the public page.
- Push the public JSON **as-is** into `job.events` (they only backfill `type` from `event_type` / `kind`).

`TrainingRuntime::job_events` (same file, ~258):

```rust
event.get("sequence").and_then(Value::as_u64).is_some_and(|sequence| sequence > after)
```

Public pages emit `sequence_number`, not `sequence`. So `watch_job` → `events_after` can return **zero events** even while the public service is streaming a live journal. The run can still complete (`drive_hosted_*` maps remote `completed` → sidecar `succeeded`), the visual opens, and `metric_points` stays empty.

**Fix (Workshop):** when copying a hosted event, also set `sequence` from `sequence_number`. Make `job_events` accept `sequence` **or** `sequence_number`. Add a unit test with a public-shaped event (`sequence_number: 1`, no `sequence`) and assert `events_after` returns it.

Also normalize `type` (already done) so `watch_job` and `append_mapped_event` see `sft.step.metrics` / `cispo.update.completed`.

### Mapping once events arrive

| Public kind | Mapped Workshop type | Kernel | Collection |
|---|---|---|---|
| `sft.step.metrics` | `sft.training.metrics` | `kernel/algorithms/sft.rs` | `metric_points` |
| `cispo.update.completed` | `training.metrics` | `kernel/algorithms/cispo.rs` (`training.metrics`) | `metric_points` |
| `cispo.clip.identity` | `cispo.clip.identity` | clip identity on CISPO state | visual chrome |
| `sft.checkpoint.created` / `cispo.checkpoint.created` | checkpoint-ready | `candidates` | `candidates` |
| `*.eval.completed` | `training.evaluation.completed` | `evaluations` | `evaluations` |

Public nested `metrics.loss` is flattened on the Python page to `train_loss` / `trainLoss`. Keep that.

**Visual landmine:** both live shells drop points with `step <= 0`:

```ts
.filter((point) => point.step > 0)
```

If a mapped metric uses `update` and `step` is missing, CISPO already falls back to `point.update`. SFT must have `step`. If fixture steps are 1-based you are fine; a zero-indexed first point will never draw.

---

## Operator setup for a Desktop click

Launch **Desktop from a terminal** so `std::env::var` sees these. A Dock/Finder launch will not inherit your shell.

```bash
export SYNTH_OPTIMIZERS_SFT_SERVICE_URL=http://127.0.0.1:8878
export SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN=local-qa-token
export SYNTH_OPTIMIZERS_CISPO_SERVICE_URL=http://127.0.0.1:8880
export SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN=local-qa-token
export SYNTH_OPTIMIZERS_SFT_FIXTURE=1          # unpaid; lives on the *service* process
export SYNTH_OPTIMIZERS_CISPO_FIXTURE=1        # unpaid; lives on the *service* process
export TINKER_CISPO_VALIDATION_RECEIPT=/Users/joshuapurtell/GitHub/optimizers/docs/receipts/tinker-gpt-oss-20b-banking77-canary-cispo/cispo.slime.v1.receipt.json

# then start :8878 / :8880 as above, start the Optimizers sidecar from the app
# (GEPA plugin must reach ready — create_and_watch calls require_plugin_ready)
```

Hosted CISPO recipe is `available` only when **both** are true in the Desktop process:

1. Receipt admits (`hosted_cispo_receipt_admits` in `sidecar_training.rs`)
2. `CispoOptimizerClient::from_env()` succeeds (token required; URL defaults to `:8880`)

Hosted Banking77 SFT recipe is `available` only when:

1. Tinker base-model catalog loads
2. `SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN` is set
3. `SYNTH_SFT_BANKING77_TRAIN_JSONL` points at a real JSONL file

Classify on `:8110` is listed as a prerequisite of the **Desktop SFT recipe TOML** (checkpoint campaigns). It is **not** required for `sft.step.metrics` / train-loss. If the hosted TOML submit fails without `:8110`, either skip eval in fixture or add a bounded fixture SFT recipe that still uses the public service. Do not block the visual on classify.

Craftax hosted SFT (`sft.craftax.nemotron-nano.tinker.v1`) additionally wants GameBench gold on `:8098`. Out of scope unless you explicitly take Craftax.

UI still disables hosted buttons with `pluginBlocked`. That is currently honest: `create_and_watch` requires the Optimizers sidecar even though training compute is on `:8878`/`:8880`. If the sidecar is down, the click will fail inside `require_training_ready`.

---

## Click path and what “done” looks like

1. Optimizers plugin = ready.
2. Launch tab → **Hosted CISPO** (`data-testid="start-cispo-hosted"`) first (no JSONL gate).
3. Visual `optimizer.cispo.live.v1` opens (`openVisual: true`).
4. While the run status is still `running`:
   - collection `metric_points` gains rows
   - clip identity shows 0 / 5 (`cispo.clip.identity`)
   - `rollouts` / `evaluations` may follow; curves must not wait for terminal
5. Repeat with **Hosted SFT** (`start-sft-hosted`) → `optimizer.sft.live.v1` train-loss series.

Debug order if the visual is empty:

1. Public page: `curl -s -H "Authorization: Bearer local-qa-token" "http://127.0.0.1:8880/v1/runs/$ID/optimizer-events?after_sequence=0"` — if this is empty, it is not a Workshop bug.
2. Sidecar training job events: `GET {sidecar}/v1/training/jobs/$ID/events?after=0` — if public is full and this is empty, it is the `sequence` vs `sequence_number` hole above.
3. Local journal / kernel: mapped types `sft.training.metrics` / `training.metrics` / `cispo.clip.identity`.
4. Renderer: `useCollectionPage(..., "metric_points")`. If the journal has mapped metrics and the collection page is empty, it is a kernel projection bug, not a producer bug.

Never rebuild charts from `events` in the live shell.

---

## Files that are yours

| Area | Path |
|---|---|
| Two-hop watch (the hole) | `apps/synth_desktop/src-tauri/src/optimizers/sidecar_training.rs` (`job_events`, `drive_hosted_sft_job`, `drive_hosted_cispo_job`, `watch_job`, `append_mapped_event`) |
| Mapping | `.../training_adapter.rs` (preferred) + fallback in `sidecar_training.rs` |
| Hosted CISPO launch | `.../cispo.rs` (`start_hosted`, `hosted_cispo_config_json`, catalog) |
| Hosted SFT launch | `.../hosted_sft.rs` |
| Public HTTP clients | `.../sft_client.rs`, `.../cispo_client.rs` (`health()` exists, catalog does not ping) |
| Dispatch | `.../service.rs` `start_recipe` |
| Kernel | `.../kernel/algorithms/sft.rs`, `.../kernel/algorithms/cispo.rs` |
| Visuals | `visuals/families/optimizers/sft/optimizer.sft.live.v1/shell.tsx`, `.../cispo/optimizer.cispo.live.v1/shell.tsx` |
| Live subscribe | `apps/synth_desktop/src/renderer/src/runtime/runProgress/subscription.ts` |
| Launch UI | `.../components/OptimizersPage.tsx`, `TrainingWorkspace.tsx` |
| Recipe ids | `sidecar_training.rs` `HOSTED_BANKING77_CISPO_RECIPE`, `HOSTED_CISPO_RECIPE`; `hosted_sft.rs` `HOSTED_SFT_BANKING77_RECIPE` |

Do **not** grow `optimizers/src/synth_optimizers/cli.py`. CISPO CLI is `cispo_cli.py` / `synth-optimizers-cispo`.

---

## Suggested implementation order

1. **Prove the two-hop.** Unit-test `job_events` with `sequence_number` only. Copy `sequence` on hosted ingest. Re-run `cargo test --lib sidecar_training`.
2. **Env + click CISPO.** Sidecar ready, receipt + token in the Desktop process, fixture CISPO service up. Click `start-cispo-hosted`. Watch `metric_points` while `running`.
3. **Click SFT.** Either set `SYNTH_SFT_BANKING77_TRAIN_JSONL` or add a fixture-friendly hosted SFT recipe that uses service-side examples (the probe TOML is `optimizers/.live-qa/sft.banking77.toml`). Train-loss must not require `:8110`.
4. **Optional polish.** Catalog `GET /health` so availability says “public SFT service not listening” instead of a later connect error. Drop leftover copy `Hosted CISPO slime` on the resource title in `cispo.rs`. Do not auto-spawn `:8878`/`:8880` unless that is explicitly in scope.

Paid Tinker is **after** the visual is live on fixture: drop `*_FIXTURE` on the **service** processes, set `TINKER_API_KEY` there, keep the existing CISPO receipt. No second canary.

---

## Constraints Guy must not violate

- File work only under `/Users/joshuapurtell/GitHub`.
- No `optimizers-beta`, no Keychain, no writing `SYNTH_OPTIMIZERS_*` into `os.environ` under `optimizers/src/` except `SYNTH_OPTIMIZERS_TERMINAL`.
- Do not consume Python SSE in the visual.
- Do not invent Craftax worlds.
- Do not rename `cispo.slime.v1` / `slime-reference` in `config_json`.
- Do not treat MLX (`cispo.mlx.v1`) as the hosted Tinker story.
- `cli.py` / `hosted.py` / `o11y.py` in `optimizers` are allowlisted shrink-only.

---

## Acceptance

Done when, on fixture services, without paid Tinker:

1. Desktop hosted CISPO recipe is `available`, click opens `optimizer.cispo.live.v1`, clip identity 0…5 appears, `metric_points` grows **before** terminal.
2. Desktop hosted SFT recipe (or a documented fixture sibling) opens `optimizer.sft.live.v1` and train loss grows **before** terminal.
3. Killing an SSE client does not stop either job.
4. Visuals never rebuild those curves from the raw journal.
