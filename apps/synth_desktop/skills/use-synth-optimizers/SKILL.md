---
name: use-synth-optimizers
description: Start, inspect, follow, reconcile, cancel, and visualize first-class Synth optimizer runs through CoreRuntime. Use for GEPA, GELO/Go-Ex, SFT, CISPO, and local eval. Source-declared recipes come from workshop.recipe.toml under configured desktop roots; do not name shipped Banking77 or Craftax GEPA/eval ids.
---

# Use Synth Optimizers

Evaluation is inline-first admission, not an optimizer algorithm and not a
catalog lookup. For an ordinary evaluation, construct a typed inline request
from the user's exact container, policy, model, seeds, and hard limits. Catalog
recipes are presets used only when the user explicitly asks for catalog
resolution.

When a harness installs policy code, include its explicit repository-relative
`policySourcePath`. Admission pins the exact bytes at the container's declared
Git revision; it never guesses a path or reads a mutable working-tree fallback.

Call `evaluation_spec_draft` (or `evaluation_spec_admit`) before spend, inspect
the returned immutable digest and approval disclosure, then call
`evaluation_start`. A missing catalog id is irrelevant to inline admission.
If admission fails, act on its structured component error such as
`evaluator_not_declared`; do not substitute another container, evaluator,
model, protocol, or execution path.

Completed optimizer evidence crosses instance boundaries only through typed
immutable snapshots. Call `export_snapshot` in the owning instance, then
`import_snapshot` in the destination and bind the returned
`optimizer_snapshot` id. Do not copy optimizer SQLite rows, follow another
instance's live database, or relabel an unresolved run binding as evidence.

Use `mcp__synth_optimizers__optimizer_manage`. Treat returned run IDs and cursors as authoritative. Never launch an optimizer with a shell command supplied by chat, accept arbitrary config for a local recipe, request credentials in chat, or reproduce secrets and signed URLs. For a product recipe, prefer `start_workflow`: the host refreshes relevant container capabilities, performs bounded approval and sidecar admission, creates the run, and opens its chat-owned visual in one call. Do not inspect the filesystem or start plugins manually before trying it.

## Choose a workflow

1. For evaluation, do not call `list_recipes` unless the user explicitly asks for a catalog preset. Use container discovery, then `evaluation_spec_draft`. For optimization and training workflows, call `list_algorithms` and `list_recipes` as applicable.
2. Choose the algorithm from the user's objective:
   - GEPA: improve prompts or other candidate values against a source-declared container. Read [references/gepa.md](references/gepa.md). Never start `gepa.banking77.*` or `gepa.craftax.*` as product ids.
   - GELO / Go-Ex: explore a hosted search space or reconcile an existing hosted run. Read [references/gelo.md](references/gelo.md).
   - Eval: score a registered container through inline admission. Supply exact policy/model pins, seeds, rollout/call/step limits, and a hard cost ceiling. Stage candidates only for candidate-comparison evals. Read [references/eval.md](references/eval.md).
   - SFT: train and compare model weights/checkpoints. Local MLX is `sft.qwen35-2b.mlx.v1` (This Mac). Hosted recipes use the public `synth-optimizers` SFT service through the Optimizers sidecar — never dial `:8787` or `:8878` from a shell. Student ids: `docs/sft_tinker_base_models.toml`. Read [references/sft.md](references/sft.md).
   - CISPO: on-policy training. Local MLX is `cispo.banking77.mlx.v1`. Hosted slime.v1 is `cispo.slime.hosted.v1` and stays unavailable until the clip-identity canary admits it. Read [references/cispo.md](references/cispo.md). PPO is not a local/hosted picker option.
3. For a local recipe, report its availability, exact fixed inputs, hard limits, prerequisite services, credential names, and whether its cost is dollar-capped or only compute-bounded.
4. Start a bounded product recipe with `start_workflow`. It returns the authoritative run, visual references, event cursor, and admission status. The host owns approval, fresh capability observation, sidecar readiness, and visual opening.
   - Recipe identity is exact. If the requested recipe is unavailable, stop and present its structured readiness blocker. Never substitute another algorithm family, another recipe, a hand-built rollout loop, or a shell workflow. In particular, an unavailable `eval.craftax.*` recipe must never become `gepa.craftax.*`.
   - The advanced/recovery sequence remains `prepare` → `open_visual` → `await_ready` → `start`. Use it only when resuming an already-prepared run or diagnosing a structured admission blocker. `start` requires a visual readiness receipt and a separate compute approval bound to the prepared run.
   - Local `eval.*` recipes are the explicit exception: stage candidates, then call `start_recipe` with the returned `candidate_set_id`. They do not install or depend on the Optimizers plugin, and the pinned target plus fixed recipe owns the compute bounds.
   - `open_visual` owns and configures the product visual. Do not call `authoring_context`, `capture_review`, `review`, `update`, or `mark_ready` for it.
   - If the first `await_ready` reports that no receipt was posted, call `mcp__synth_visuals__visual_manage` once with `operation: "show"` and the run's primary visual ID, then retry `await_ready`. Do not inspect processes, environment variables, source files, databases, or IPC files to manufacture readiness.
   - Preserve the exact `preparationDigest` returned by `prepare` and pass it as `preparation_digest` with `optimizer_run_id` on the first `start` call. Never request approval with a missing or reconstructed digest.
5. Pass the catalog `recipe_id` to `start_workflow`. For candidate-comparison `eval.*` recipes, also pass the `candidate_set_id` returned by `stage_eval_candidates`. For source-declared baseline evals, pass `container_id` from container ensure when more than one healthy pool is registered. The Rust host owns commands, paths, hyperparameters, capability refresh, and credential resolution. Retrieve the winner with `get_result` — never read result files by filesystem path.

## Follow every run

1. For a run started from chat, pass `open_visual: true`. The host creates and binds the algorithm-family visual before starting compute, reuses one durable visual ID, and shows it in the current conversation's right pane. Do not create a second generic visual for the same run.
2. For an existing or historical run, call `open_visual` with its `optimizer_run_id`. This reuses its primary visual and presents it in the current conversation without changing the run's original ownership.
3. Record `run.id`, the primary visual ID in `run.visualRefs`, and `run.cursorSeq`. Keep the pane open while following the run; the visual reads the same durable event cursor and continues updating independently of tool polling.
   After the run ID is known, call `mcp__synth_session__session_present` once with a concise title containing the task family and the run ID's final 6 characters, for example `Banking77 eval · a1b2c3`. This is the current conversation's scoped title MCP; it prevents concurrent or restored runs from becoming indistinguishable. Do not pass a session ID and do not rename another conversation.
4. Call `watch_run` with `optimizer_run_id` and `after_seq` equal to the last processed sequence. Advance to the greatest returned sequence. Empty batches are normal.
   - Wait for progress only by calling `watch_run` again (or `get_run` when a status snapshot is useful). Never run a shell or terminal command, including `sleep`, just to delay or poll an optimizer run; repeated optimizer MCP calls are the supported waiting mechanism.
5. Use `get_run` for status and summary, and `get_state` for the algorithm-specific slices in its reference.
6. Stop only at `completed`, `failed`, `cancelled`, or `degraded`. Use `cancel_run` only when the user requests it.
7. After a Desktop restart, recover with `list_runs`/`get_run`, call `open_visual`, and continue from the persisted cursor. Reconcile cloud runs before watching them. Local process records and events survive restart, but a process owned by the previous Desktop session is not reattached.
8. To chat with a catalog LoRA, `list_checkpoints` then `infer_checkpoint` with `family=chat_completions|responses` and a native OpenAI body. Never wrap `{message, reply}` or name mlx-rl, Tinker, or `:8787`.

## Present the result

Show the visual before a chat-started run and whenever the user asks to inspect an existing run. The pane and the chat artifact must reference the same visual ID. Report:

- algorithm, objective, run ID, source, execution binding, status, and final cursor;
- declared limits versus actual usage;
- algorithm-specific winner, uplift, frontier, or checkpoint evidence;
- artifact titles and visual ID;
- bounded failure diagnostic and log filename when failed.

Distinguish measurement-only held-out evaluations from evidence used for selection or promotion. For `eval`, report the run status and selection status separately: a completed run that promoted nothing is a result, not a failure.
For GEPA, do not stop at `get_result`: retrieve both `gepa.candidates` and `gepa.frontier`, then explain the selected candidate against the seed and other proposals using the available train, minibatch, held-out, frontier, and rejection evidence.
