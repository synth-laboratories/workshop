# SFT

Use SFT when the output is a trained adapter/checkpoint rather than a prompt candidate. SFT is a **training** algorithm. Local vs hosted is recipe placement, admitted by the Optimizers sidecar. Never dial `127.0.0.1:8787`, `127.0.0.1:8878`, or name `synth-mlx-rl` in a shell. The agent only sees `recipe_id` → `optimizer_run_id` → `watch_run` / `open_visual`.

Workshop mirrors `optimizer_event_page.v1` and opens `optimizer.sft.live.v1`. Held-out evaluation arrives as `sft.heldout_evaluation.completed`.

## This Mac (MLX · Qwen 0.8B)

Recipe: `sft.qwen35-0.8b.mlx.v1`.

Requires the Optimizers plugin/sidecar. Datasets come from a ready container's advertised SFT JSONL routes (`/workshop/manifest` or `optimizer_contracts.sft`) or from `SYNTH_MLX_SFT_TRAIN_JSONL` / `SYNTH_MLX_SFT_EVAL_JSONL`. Missing real datasets fail closed. Apple Silicon. The sidecar starts and probes `synth-mlx-rl`; do not tell a shell to dial `:8787`. No hosted provider charges.

After explicit user instruction:

```json
{"operation":"start_workflow","arguments":{"recipe_id":"sft.qwen35-0.8b.mlx.v1","open_visual":true,"container_id":"ctr_..."}}
```

Follow training metrics, `sft.checkpoint.ready`, and the paired `sft.heldout_evaluation.completed` receipt. Resume uses `resume_run`. Chat Completions and Responses against a catalog LoRA use `optimizer_manage` `infer_checkpoint` (`family=chat_completions|responses`) after `list_checkpoints`. Never wrap a `{message, reply}` helper and never name mlx-rl.

`get_result` for SFT is typed from the durable event stream. It does not read `best_candidate.json` and is not GEPA-shaped. Missing scores stay `—`, never `0`.

## Hosted Tinker SFT

Recipe: `sft.hosted.tinker.v1`. Available when `SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN` is set (URL defaults to `http://127.0.0.1:8878`). Student ids: workshop `docs/sft_tinker_base_models.toml`. Optional `base_model` must be an id from that file. Bind evaluation with `SYNTH_SFT_EVAL_CONTAINER_URL`, `SYNTH_SFT_EVAL_PLAN_REF`, `SYNTH_SFT_EVAL_WORLD_REF`, and optional `SYNTH_SFT_EVAL_HARNESS`. Training rows come from `SYNTH_SFT_TRAIN_JSONL`. The public service owns any private executor handoff. Spec: workshop `docs/optimizers_beta_sft.md`. Tinker charges apply; say that before asking for approval.

After explicit approval:

```json
{"operation":"start_workflow","arguments":{"recipe_id":"sft.hosted.tinker.v1","open_visual":true}}
```

