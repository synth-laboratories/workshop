# GEPA

Use GEPA for prompt/candidate optimization with rollout-based evaluation.

Workshop does not ship task GEPA recipes. Fresh `list_recipes` has no
`gepa.banking77.*` or `gepa.craftax.*` ids. A GEPA run exists only after the
configured desktop source root declares it in `workshop.recipe.toml` (or
`workshop.recipes/*.toml`) with `algorithm = "gepa"` and a container from
`workshop.containers.toml`. Author the target with `$author-synth-container`
when the catalog has none. A session never selects the source root.

## Before starting

Call `list_recipes` and report the selected source recipe's catalog values
exactly: id, container, locality, bounds (`max_cost_usd`, `max_total_rollouts`).
Product caps are `$2.45` and `240` rollouts; the source declaration may be stricter,
never looser. The trusted Desktop OpenAI credential is required. Do not look
for a packaged cookbook.

Use the one-shot workflow admission path. Pass the catalog `recipe_id` and
`container_id` from `container_ensure` when more than one healthy pool
advertises that family. Do not pass URLs, paths, commands, or credentials:

```json
{"operation":"start_workflow","arguments":{"recipe_id":"<catalog recipe id>","open_visual":true}}
{"operation":"get_result","arguments":{"optimizer_run_id":"<id>"}}
```

`start_recipe` is the advanced prepare-only path. Prefer `start_workflow`, which
performs bounded host approval and sidecar admission before starting compute.
Retrieve the winner with `get_result` — never read `best_candidate.json` by
filesystem path.

Follow `gepa.candidates`, `gepa.frontier`, and `gepa.reflections`. At terminal
state, explicitly retrieve `gepa.candidates` and `gepa.frontier` before
`get_result`. Report the selected candidate, its materialized prompt, how it
differs from the seed and other proposals, train/minibatch and measurement-only
held-out scores, frontier membership, rejection reason when present, rollout
usage, cost, and candidate/result artifacts. Prefer the visual for comparing
candidates; candidates and artifacts support copy/download.

For non-recipe GEPA runs, use `import_local` through the full optimizer tools or
reconcile a hosted run, then follow the same slices.
