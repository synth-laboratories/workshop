# CISPO

Use CISPO when the user wants on-policy training with the pinned slime CISPO objective, not SFT and not GEPA search. Local vs hosted is recipe placement. Never dial `:8787`, Tinker, or a container from a shell. The Optimizers sidecar admits the recipe and owns the tunnel lease for hosted runs.

## This Mac (MLX · Banking77)

Recipe: `cispo.banking77.mlx.v1`.

Requires the Optimizers sidecar on Apple Silicon. Warm-start from an SFT adapter on that task when possible; otherwise the visual reports `cispo_no_learning_signal` instead of crashing.

```json
{"operation":"start_workflow","arguments":{"recipe_id":"cispo.banking77.mlx.v1","open_visual":true}}
```

## Hosted Tinker

Recipe: `cispo.banking77.tinker.v1` (alias `cispo.hosted.tinker.v1`). Fail-closed until `TINKER_CISPO_VALIDATION_RECEIPT` admits hosted CISPO. Public `synth-optimizers` CISPO service on Tinker — not Modal slime. Do not draft a free-form agent launch.

```json
{"operation":"start_workflow","arguments":{"recipe_id":"cispo.banking77.tinker.v1","open_visual":true}}
```
