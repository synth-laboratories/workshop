# CISPO

Use CISPO when the user wants on-policy training with the pinned slime CISPO objective, not SFT and not GEPA search. Local vs hosted is recipe placement. Never dial `:8787`, Tinker, or a container from a shell. The Optimizers sidecar admits the recipe and owns the tunnel lease for hosted runs.

## This Mac (MLX · Banking77)

Recipe: `cispo.banking77.mlx.v1`.

Requires the Optimizers sidecar on Apple Silicon. Warm-start from an SFT adapter on that task when possible; otherwise the visual reports `cispo_no_learning_signal` instead of crashing.

```json
{"operation":"start_workflow","arguments":{"recipe_id":"cispo.banking77.mlx.v1","open_visual":true}}
```

## Hosted slime.v1

Recipe: `cispo.slime.hosted.v1`. Fail-closed until the slime clip identity canary (`1 + eps_high`) admits `training.cispo.hosted`. Do not draft a free-form agent launch against Optimizers-beta.

```json
{"operation":"start_workflow","arguments":{"recipe_id":"cispo.slime.hosted.v1","open_visual":true}}
```
