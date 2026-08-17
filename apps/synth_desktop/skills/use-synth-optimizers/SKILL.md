---
name: use-synth-optimizers
description: Start, inspect, follow, reconcile, cancel, and visualize first-class Synth optimizer runs through CoreRuntime. Use for GEPA prompt optimization, GELO/Go-Ex exploration, SFT/fine-tuning, local eval scoring of policy candidates, Banking77, Craftax GPT-OSS, GameBench/Harbor, optimizer recipes, candidates, scorecards, frontiers, checkpoints, evaluations, artifacts, usage, diagnostics, or optimizer state slices.
---

# Use Synth Optimizers

Use `mcp__synth_optimizers__optimizer_manage`. Treat returned run IDs and cursors as authoritative. Never launch an optimizer with a shell command supplied by chat, accept arbitrary config for a local recipe, request credentials in chat, or reproduce secrets and signed URLs. If the Optimizers plugin is not ready, the tool returns `plugin_not_ready` — call `mcp__synth_plugins__plugin_manage` to install/start it. Do not expect optimizer tools to download the sidecar.

## Choose a workflow

1. Call `list_algorithms` and `list_recipes` before proposing a run.
2. Choose the algorithm from the user's objective:
   - GEPA: improve prompts or other candidate values. The pinned smokes are `gepa.banking77.smoke.v1` and `gepa.craftax.smoke.v1`; read [references/gepa.md](references/gepa.md).
   - GELO / Go-Ex: explore a hosted search space or reconcile an existing hosted run. Read [references/gelo.md](references/gelo.md).
   - Eval: score several policy variants against a pinned evaluation container and pick a winner. Stage candidates first; `start_recipe` takes a `candidate_set_id`, never a path. Read [references/eval.md](references/eval.md).
   - SFT: train and compare model weights/checkpoints. All hosted SFT recipes, including `sft.hosted.fixture.v1` and `sft.craftax.nemotron-nano.tinker.v1`, use the public `synth-optimizers` SFT service; Workshop never contacts the private training executor. The separate local Tinker smoke is `sft.craftax.gpt-oss.smoke.v1`. Student ids: `docs/sft_tinker_base_models.toml`. Read [references/sft.md](references/sft.md).
3. For a local recipe, report its availability, exact fixed inputs, hard limits, prerequisite services, credential names, and whether its cost is dollar-capped or only compute-bounded.
4. Enforced connect-before-start: `prepare` → `open_visual` → `await_ready` → `start`. `start` requires a visual readiness receipt and a separate compute approval bound to the prepared run. Listing, importing, reconciling, inspecting, and visualizing do not require compute approval.
   - Local `eval.*` recipes are the explicit exception: stage candidates, then call `start_recipe` with the returned `candidate_set_id`. They do not install or depend on the Optimizers plugin, and the pinned target plus fixed recipe owns the compute bounds.
   - `open_visual` owns and configures the product visual. Do not call `authoring_context`, `capture_review`, `review`, `update`, or `mark_ready` for it. Report `summary.visualEvidence.state` (`ready` | `reviewed` | `partial` | `failed`) at terminal; never loop capture/repair. `partial` and `failed` never block task completion.
   - If the first `await_ready` reports that no receipt was posted, call `mcp__synth_visuals__visual_manage` once with `operation: "show"` and the run's primary visual ID, then retry `await_ready`. Do not inspect processes, environment variables, source files, databases, or IPC files to manufacture readiness.
   - Preserve the exact `preparationDigest` returned by `prepare` and pass it as `preparation_digest` with `optimizer_run_id` on the first `start` call. Never request approval with a missing or reconstructed digest.
5. Pass only `recipe_id` to `prepare`; for `eval.*`, pass only `recipe_id` plus the `candidate_set_id` returned by `stage_eval_candidates`. The Rust host owns commands, paths, hyperparameters, and credential resolution. Retrieve the winner with `get_result` — never read result files by filesystem path.

## Follow every run

1. For a run started from chat, pass `open_visual: true`. The host creates and binds the algorithm-family visual before starting compute, reuses one durable visual ID, and shows it in the current conversation's right pane. Do not create a second generic visual for the same run.
2. For an existing or historical run, call `open_visual` with its `optimizer_run_id`. This reuses its primary visual and presents it in the current conversation without changing the run's original ownership.
3. Record `run.id`, the primary visual ID in `run.visualRefs`, and `run.cursorSeq`. Keep the pane open while following the run; the visual reads the same durable event cursor and continues updating independently of tool polling.
4. Call `watch_run` with `optimizer_run_id` and `after_seq` equal to the last processed sequence. Advance to the greatest returned sequence. Empty batches are normal.
   - Wait for progress only by calling `watch_run` again (or `get_run` when a status snapshot is useful). Never run a shell or terminal command, including `sleep`, just to delay or poll an optimizer run; repeated optimizer MCP calls are the supported waiting mechanism.
5. Use `get_run` for status and summary, and `get_state` for the algorithm-specific slices in its reference.
6. Stop only at `completed`, `failed`, or `cancelled`. Use `cancel_run` only when the user requests it.
7. After a Desktop restart, recover with `list_runs`/`get_run`, call `open_visual`, and continue from the persisted cursor. Reconcile cloud runs before watching them. Local process records and events survive restart, but a process owned by the previous Desktop session is not reattached.

## Present the result

Show the visual before a chat-started run and whenever the user asks to inspect an existing run. The pane and the chat artifact must reference the same visual ID. Report:

- algorithm, objective, run ID, source, execution binding, status, and final cursor;
- `summary.visualEvidence.state` (`ready` | `reviewed` | `partial` | `failed`) — never loop capture/repair, and never hold the turn in Working because the visual is `partial` or `failed`;
- declared limits versus actual usage;
- algorithm-specific winner, uplift, frontier, or checkpoint evidence;
- artifact titles and visual ID;
- bounded failure diagnostic and log filename when failed.

Distinguish measurement-only held-out evaluations from evidence used for selection or promotion. For `eval`, report the run status and selection status separately: a completed run that promoted nothing is a result, not a failure.
For GEPA, do not stop at `get_result`: retrieve both `gepa.candidates` and `gepa.frontier`, then explain the selected candidate against the seed and other proposals using the available train, minibatch, held-out, frontier, and rejection evidence.
