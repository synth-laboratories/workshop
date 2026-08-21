# SFT

Use SFT when the output is a trained adapter/checkpoint rather than a prompt candidate. SFT is a **training** algorithm. Local vs hosted is recipe placement, admitted by the Optimizers sidecar. Never dial `127.0.0.1:8787`, `127.0.0.1:8878`, or name `synth-mlx-rl` in a shell. The agent only sees `recipe_id` → `optimizer_run_id` → `watch_run` / `open_visual`.

Workshop mirrors `optimizer_event_page.v1` and opens `optimizer.sft.live.v1`. Held-out evaluation arrives as `sft.heldout_evaluation.completed`.

## This Mac (MLX · Qwen 0.8B)

Recipe: `sft.qwen35-0.8b.mlx.v1`.

Requires the Optimizers plugin/sidecar. Datasets: cookbook `cookbooks/optimizers/sft/qwen35_mlx/{train,eval}.jsonl` or explicit `SYNTH_MLX_SFT_TRAIN_JSONL` / `SYNTH_MLX_SFT_EVAL_JSONL`. Missing real datasets fail closed. Apple Silicon. The sidecar starts and probes `synth-mlx-rl`; do not tell a shell to dial `:8787`. No hosted provider charges.

After explicit user instruction:

```json
{"operation":"start_workflow","arguments":{"recipe_id":"sft.qwen35-0.8b.mlx.v1","open_visual":true}}
```

Follow training metrics, `sft.checkpoint.ready`, and the paired `sft.heldout_evaluation.completed` receipt. Resume uses `resume_run`. Chat Completions and Responses against a catalog LoRA use `optimizer_manage` `infer_checkpoint` (`family=chat_completions|responses`) after `list_checkpoints`. Never wrap a `{message, reply}` helper and never name mlx-rl.

`get_result` for SFT is typed from the durable event stream. It does not read `best_candidate.json` and is not GEPA-shaped. Missing scores stay `—`, never `0`.

## Craftax Nemotron 3.5 Lightning Tinker (hosted, local Craftax slot)

Recipe: `sft.craftax.nemotron-nano.tinker.v1`. Available when `SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN` is set (URL defaults to `http://127.0.0.1:8878`). Student ids: workshop `docs/sft_tinker_base_models.toml` (default is 3.5 Lightning). Optional `base_model` must be an id from that file. Checkpoint evals hit the local Craftax slot (`CRAFTAX_URL` or `http://127.0.0.1:8098`). Training rows come from `SYNTH_SFT_TRAIN_JSONL` on Desktop and are copied into the product-owned recipe. The public service owns any private executor handoff. Spec: workshop `docs/optimizers_beta_sft.md`. Do not present `goex.sft.v1` as this recipe. Tinker charges apply; say that before asking for approval.

After explicit approval:

```json
{"operation":"start_workflow","arguments":{"recipe_id":"sft.craftax.nemotron-nano.tinker.v1","open_visual":true}}
```

## Start the pinned Craftax GPT-OSS smoke

Recipe: `sft.craftax.gpt-oss.smoke.v1`.

The recipe fixes:

- teacher: `openai/gpt-oss-120b` via Groq, seeds 101–104;
- student: `openai/gpt-oss-20b` Tinker LoRA, rank 8 (`lora_r8`), batch size 2, 4 training steps;
- held-out comparison: seeds 501–502, each evaluated on base and adapter;
- ceilings: 4 teacher rollouts, 4 evaluation rollouts, 8 total environment rollouts.

It requires the trusted Craftax binary and bridge runtime plus `GROQ_API_KEY` and `TINKER_API_KEY`. The Rust host reuses Craftax at `127.0.0.1:8098` when present; otherwise it starts and owns the trusted binary for the duration of the run. Provider charges apply. This smoke is bounded by fixed rollouts and steps, not by a dollar ceiling; say that plainly before asking for approval.

After explicit approval:

```json
{"operation":"start_workflow","arguments":{"recipe_id":"sft.craftax.gpt-oss.smoke.v1","open_visual":true}}
```

Follow these slices:

- `sft.dataset`: collected training row count and held-out split;
- `sft.training_curves`: completed steps and learning rate;
- `sft.checkpoints`: Tinker sampler/state lineage;
- `sft.checkpoint_evaluations`: per-seed base and SFT rewards plus final uplift;
- `sft.examples`: example comparisons when emitted;
- `sft.compute`, `run.usage`, and `run.artifacts`.

Expected artifacts are `train.jsonl`, `train_result.json`, `eval_summary.json`, stdout, and stderr. Report base mean, SFT mean, uplift, adapter/checkpoint path label, row count, completed steps, rollout usage, and whether all artifacts materialized. Held-out evaluation is measurement-only and must not be described as checkpoint-selection evidence.
