# SFT

Use SFT when the output is a trained adapter/checkpoint rather than a prompt candidate. Hosted SFT lives in the public `synth-optimizers` service (`algorithm_id: "sft"`), not in Workshop's private beta client and not in `goex.sft.v1`. Workshop mirrors `optimizer_event_page.v1` and opens `optimizer.sft.live.v1`.

## Start the hosted fixture (streaming)

Recipe: `sft.hosted.fixture.v1`.

Requires the public service plus `SYNTH_OPTIMIZERS_SFT_SERVICE_URL` (defaults to `http://127.0.0.1:8878`) and `SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN`. The fixture backend charges nothing. Each explicit start creates a distinct canonical public run.

After explicit user instruction (no paid-compute approval is needed for this no-cost fixture):

```json
{"operation":"start_recipe","arguments":{"recipe_id":"sft.hosted.fixture.v1","open_visual":true}}
```

Follow `watch_run` from sequence 0. Expect `optimizer.visual.ready`, then `sft.training.metrics` (not `sft.step.metrics`). Null `validation_loss` stays missing (`—`). `sft.checkpoint.ready` is not promotion. Checkpoint-eval children start without reward/cost; `sft.checkpoint_rollout.completed` patches those fields. Missing stays `—`.

## Craftax Nemotron 3.5 Lightning Tinker (hosted, local Craftax slot)

Recipe: `sft.craftax.nemotron-nano.tinker.v1`. Available when `SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN` is set (URL defaults to `http://127.0.0.1:8878`). Student ids: workshop `docs/sft_tinker_base_models.toml` (default is 3.5 Lightning). Optional `base_model` must be an id from that file. Checkpoint evals hit the local Craftax slot (`CRAFTAX_URL` or `http://127.0.0.1:8098`). Training rows come from `SYNTH_SFT_TRAIN_JSONL` on Desktop and are copied into the product-owned recipe. The public service owns any private executor handoff. Spec: workshop `docs/optimizers_beta_sft.md`. Do not present `goex.sft.v1` as this recipe. Tinker charges apply; say that before asking for approval.

After explicit approval:

```json
{"operation":"start_recipe","arguments":{"recipe_id":"sft.craftax.nemotron-nano.tinker.v1","open_visual":true}}
```

## Start the pinned Craftax GPT-OSS smoke

Recipe: `sft.craftax.gpt-oss.smoke.v1`.

The recipe fixes:

- teacher: `openai/gpt-oss-120b` via Groq, seeds 101–104;
- student: `openai/gpt-oss-20b` Tinker LoRA, rank 8, batch size 2, 4 training steps;
- held-out comparison: seeds 501–502, each evaluated on base and adapter;
- ceilings: 4 teacher rollouts, 4 evaluation rollouts, 8 total environment rollouts.

It requires the trusted Craftax binary and bridge runtime plus `GROQ_API_KEY` and `TINKER_API_KEY`. The Rust host reuses Craftax at `127.0.0.1:8098` when present; otherwise it starts and owns the trusted binary for the duration of the run. Provider charges apply. This smoke is bounded by fixed rollouts and steps, not by a dollar ceiling; say that plainly before asking for approval.

After explicit approval:

```json
{"operation":"start_recipe","arguments":{"recipe_id":"sft.craftax.gpt-oss.smoke.v1","open_visual":true}}
```

Follow these slices:

- `sft.dataset`: collected training row count and held-out split;
- `sft.training_curves`: completed steps and learning rate;
- `sft.checkpoints`: Tinker sampler/state lineage;
- `sft.checkpoint_evaluations`: per-seed base and SFT rewards plus final uplift;
- `sft.examples`: example comparisons when emitted;
- `sft.compute`, `run.usage`, and `run.artifacts`.

Expected artifacts are `train.jsonl`, `train_result.json`, `eval_summary.json`, stdout, and stderr. Report base mean, SFT mean, uplift, adapter/checkpoint path label, row count, completed steps, rollout usage, and whether all artifacts materialized. Held-out evaluation is measurement-only and must not be described as checkpoint-selection evidence.
