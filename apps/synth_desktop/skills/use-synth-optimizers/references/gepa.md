# GEPA

Use GEPA for prompt/candidate optimization with rollout-based evaluation.

## Start the pinned Banking77 smoke

Recipe: `gepa.banking77.smoke.v1`.

Before starting, report the catalog values. The current recipe fixes 4 train rows, 2 held-out rows, one generation, one proposal, at most 8 rollouts, and a $0.25 hard ceiling. It requires the Banking77 cookbook and `OPENAI_API_KEY` in the trusted Desktop environment.

After explicit approval:

```json
{"operation":"start_recipe","arguments":{"recipe_id":"gepa.banking77.smoke.v1","open_visual":true}}
```

Follow `gepa.candidates`, `gepa.frontier`, and `gepa.reflections`. Report the selected candidate, its materialized values, train and held-out scores, frontier membership, rollout usage, cost, and candidate/result artifacts. Prefer the visual for comparing candidates; candidates and artifacts support copy/download.

For non-recipe GEPA runs, use `import_local` through the full optimizer tools or reconcile a hosted run, then follow the same slices.
