# CISPO

Use CISPO when the user wants on-policy training, not SFT and not GEPA search. Local vs hosted is recipe placement. Never dial `:8787`, Tinker, or a container from a shell. The Optimizers sidecar admits the recipe and owns the tunnel lease for hosted runs.

Do not invent a per-container recipe id. The product recipe is `cispo.mlx.v1` (or hosted `cispo.slime.hosted.v1`). Workshop binds rollout URL, worlds, harness, and implementation from the registered container's `/workshop/manifest` or `/info` `optimizer_contracts.cispo`.

## This Mac (MLX)

Recipe: `cispo.mlx.v1`.

Requires the Optimizers sidecar on Apple Silicon and a ready registered container that advertises a CISPO contract. Pass `container_id` when more than one pool is registered. Warm-start from an SFT adapter when possible; otherwise the visual reports `cispo_no_learning_signal` instead of crashing.

```json
{"operation":"start_workflow","arguments":{"recipe_id":"cispo.mlx.v1","open_visual":true,"container_id":"ctr_..."}}
```

## Hosted slime.v1

Recipe: `cispo.slime.hosted.v1`. Fail-closed until the slime clip identity canary (`1 + eps_high`) admits `training.cispo.hosted`. The same container contract bind applies. Do not draft a free-form agent launch against Optimizers-beta.

```json
{"operation":"start_workflow","arguments":{"recipe_id":"cispo.slime.hosted.v1","open_visual":true,"container_id":"ctr_..."}}
```
