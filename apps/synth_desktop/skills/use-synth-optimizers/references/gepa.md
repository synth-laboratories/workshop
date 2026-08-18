# GEPA

Use GEPA for prompt/candidate optimization with rollout-based evaluation.

## Pinned end-to-end smokes

Recipes:

- `gepa.banking77.smoke.v1` optimizes a Banking77 classification prompt. It fixes 50 train rows, 20-example minibatches, 50 held-out rows per terminal candidate, one generation, and one proposal. It allows at most 240 rollouts and a $2.45 hard ceiling.
- `gepa.craftax.smoke.v1` optimizes the Craftax ReAct system prompt used by an OpenAI `gpt-4.1-nano` candidate policy. It fixes one training seed, one held-out seed, minibatches of one, eight turns per episode, one generation, and one proposal. It allows at most six rollouts and a $1.50 hard ceiling.

Before starting, report the selected recipe's catalog values exactly. Both recipes require their packaged cookbook and the trusted Desktop OpenAI credential. Banking77 evaluates prompt candidates directly; Craftax runs deterministic environment episodes and scores the candidate policy's ReAct behavior.

Use the one-shot workflow admission path. Do not pass URLs, paths, commands, or credentials:

```json
{"operation":"start_workflow","arguments":{"recipe_id":"gepa.banking77.smoke.v1","open_visual":true}}
{"operation":"get_result","arguments":{"optimizer_run_id":"<id>"}}
```

Replace the recipe ID with `gepa.craftax.smoke.v1` for Craftax. Do not add any other arguments to `prepare`.

`start_recipe` is the advanced prepare-only path. Prefer `start_workflow`, which performs bounded host approval and sidecar admission before starting compute. Retrieve the winner with `get_result` — never read `best_candidate.json` by filesystem path.

Follow `gepa.candidates`, `gepa.frontier`, and `gepa.reflections`. At terminal state, explicitly retrieve `gepa.candidates` and `gepa.frontier` before `get_result`. Report the selected candidate, its materialized prompt (`system_prompt` for Banking77 or `react_system_prompt` for Craftax), how it differs from the seed and other proposals, train/minibatch and measurement-only held-out scores, frontier membership, rejection reason when present, rollout usage, cost, and candidate/result artifacts. Prefer the visual for comparing candidates; candidates and artifacts support copy/download.

For non-recipe GEPA runs, use `import_local` through the full optimizer tools or reconcile a hosted run, then follow the same slices.
