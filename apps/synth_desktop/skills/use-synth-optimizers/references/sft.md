# SFT

Use SFT when the output is a trained adapter/checkpoint rather than a prompt candidate.

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
