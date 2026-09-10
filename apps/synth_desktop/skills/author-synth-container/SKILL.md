---
name: author-synth-container
description: Author a task container and workspace recipe from scratch, then evaluate it through Workshop. Use when the user wants a new eval or GEPA target, Banking77-from-scratch, a custom classify/chat world, or when list_recipes is empty. Never name a shipped Workshop recipe id.
---

# Author a Workshop container

Workshop does not ship task containers or cookbook recipes. The session workspace declares them. Cookbooks are a read-only pin you may copy from; they are never a spawn cwd.

## Write the service

In the workspace, author a small HTTP service that implements the container contract:

- `GET /health` → 200 when ready
- `GET /info` → family, capabilities (policy_refs, operations). Missing advertisement is a refusal, not a Workshop fallback.
- Rollout routes the family needs (`/rollouts/prepare`, start, poll) if you will run eval against the handle

Keep the process loopback-only. Do not ask Workshop to scan ports.

## Declare the process

`workshop.containers.toml`:

```toml
[[container]]
id = "classify"
command = ["python3", "serve.py"]
cwd = "."
url = "http://127.0.0.1:8099"
health = "/health"
contract = "synth-containers/v1"
locality = "container"
family = "classify"
```

`url` is required so Workshop can probe health without scanning. `cwd` must stay inside this workspace.

Call `container_ensure` with `spec_id`. It starts the command if needed, waits for `/health`, and returns `containerId`. Do not `container_register` a guessed URL after a scan.

## Declare the recipe

`workshop.recipe.toml` or `workshop.recipes/<id>.toml`:

```toml
id = "eval.classify.baseline.v1"
algorithm = "eval"
container = "classify"
provider = "openai"
model = "gpt-4.1-nano"
locality = "container"
family = "classify"
harness = "desktop_eval"
policy_config = "default"
train_seeds = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
[bounds]
max_cost_usd = 0.50
max_total_rollouts = 10
```

`locality = "container"` binds the container-reachable provider proxy (`host.docker.internal`), never `127.0.0.1`. `locality = "host"` is for processes that share the Desktop loopback. Bounds may not exceed the product cap.

## Run eval

`list_recipes` in this session should now include the workspace id. Call `start_workflow` / `optimizer_start_recipe` with that id and `container_id` from ensure. Do not pass `eval.banking77.baseline.v1` unless that string is the id you wrote in this workspace.

Proof that the cutover worked: no shipped recipe id, no cookbook cwd, no loopback policy URL for `locality=container`.
