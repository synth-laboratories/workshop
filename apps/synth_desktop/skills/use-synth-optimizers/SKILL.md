---
name: use-synth-optimizers
description: Start, inspect, follow, reconcile, cancel, and visualize first-class Synth optimizer runs through CoreRuntime. Use for GEPA prompt optimization, GELO/Go-Ex exploration, SFT/fine-tuning, Banking77, Craftax GPT-OSS, optimizer recipes, candidates, frontiers, checkpoints, evaluations, artifacts, usage, diagnostics, or optimizer state slices.
---

# Use Synth Optimizers

Use `mcp__synth_optimizers__optimizer_manage`. Treat returned run IDs and cursors as authoritative. Never launch an optimizer with a shell command supplied by chat, accept arbitrary config for a local recipe, request credentials in chat, or reproduce secrets and signed URLs.

## Choose a workflow

1. Call `list_algorithms` and `list_recipes` before proposing a run.
2. Choose the algorithm from the user's objective:
   - GEPA: improve prompts or other candidate values. The pinned smoke is `gepa.banking77.smoke.v1`; read [references/gepa.md](references/gepa.md).
   - GELO / Go-Ex: explore a hosted search space or reconcile an existing hosted run. Read [references/gelo.md](references/gelo.md).
   - SFT: train and compare model weights/checkpoints. Stream hosted SFT with `sft.hosted.fixture.v1`; the Tinker Craftax smoke is `sft.craftax.gpt-oss.smoke.v1`. The Craftax Nemotron 3.5 Lightning hosted recipe (`sft.craftax.nemotron-nano.tinker.v1`) POSTs to local/hosted optimizers-beta and evaluates against the local Craftax slot. Student ids: `docs/sft_tinker_base_models.toml`. Read [references/sft.md](references/sft.md).
3. For a local recipe, report its availability, exact fixed inputs, hard limits, prerequisite services, credential names, and whether its cost is dollar-capped or only compute-bounded.
4. Require explicit user approval before `start_recipe`. Listing, importing, reconciling, inspecting, and visualizing do not require compute approval.
5. Pass only `recipe_id`, optional `session_ref`, and `open_visual`. The Rust host owns commands, paths, hyperparameters, and credential resolution.

## Follow every run

1. Record `run.id` and `run.cursorSeq`.
2. Call `watch_run` with `optimizer_run_id` and `after_seq` equal to the last processed sequence. Advance to the greatest returned sequence. Empty batches are normal.
3. Use `get_run` for status and summary, and `get_state` for the algorithm-specific slices in its reference.
4. Stop only at `completed`, `failed`, or `cancelled`. Use `cancel_run` only when the user requests it.
5. After a Desktop restart, recover with `list_runs`/`get_run` and continue from the persisted cursor. Reconcile cloud runs before watching them. Local process records and events survive restart, but a process owned by the previous Desktop session is not reattached.

## Present the result

Open the visual when requested or when comparison is materially clearer there. Report:

- algorithm, objective, run ID, source, execution binding, status, and final cursor;
- declared limits versus actual usage;
- algorithm-specific winner, uplift, frontier, or checkpoint evidence;
- artifact titles and visual ID;
- bounded failure diagnostic and log filename when failed.

Distinguish measurement-only held-out evaluations from evidence used for selection or promotion.
