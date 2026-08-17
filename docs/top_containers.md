# Top containers to test with Workshop

These are the task services Workshop actually dogfoods. A container is a small
HTTP façade: it owns task data, policy invocation, scoring, and durable
rollout evidence. Workshop registers the URL, probes `/info`, then prepares /
starts / polls / rewards. Optimizer campaigns stay in Workshop/Optimizers.

Usual sibling checkouts on this machine:

| Repo | Path |
| --- | --- |
| Workshop | this repository |
| Containers | `../GitHub/containers` from `Documents/`, or `/Users/joshuapurtell/Documents/GitHub/containers` |
| Public cookbooks | `/Users/joshuapurtell/Documents/GitHub/synth-cookbooks-public` |

Give every Desktop instance its own `--storage-root` / `--port`. Bind loopback
only. Missing cost stays `null`; missing reward is never coerced to `0`.

## Banking77 — closed-label classify

Baseline eval: 10 train seeds, concurrency 10, environment-authored accuracy.
No candidate generation. Workshop recipe `eval.banking77.baseline.v1`.

| Role | Path |
| --- | --- |
| Target id | `banking77_classify` |
| Runtime | `containers/src/synth_containers/platform/runtimes/banking77.py` |
| Target spec | `containers/src/synth_containers/platform/targets.py` (`BANKING77_CLASSIFY`) |
| Loopback server | `containers/examples/serve_banking77.py` (default `127.0.0.1:8099`) |
| Cookbook launcher | `synth-cookbooks-public/cookbooks/optimizers/gepa/banking77_container/run_container.sh` (default `:8765`) |
| Cookbook contract | `synth-cookbooks-public/cookbooks/optimizers/gepa/banking77_container/README.md` |
| Retention test | `synth-cookbooks-public/cookbooks/optimizers/gepa/banking77_container/` → `pytest -q test_rollout_retention.py` |
| Platform tests | `containers/tests/test_banking77_platform.py` |
| Visual | `experiment.overview.v1` (baseline eval) or `live.eval_stream.v1` (single rollout) |

```bash
cd /Users/joshuapurtell/Documents/GitHub/containers
uv run python examples/serve_banking77.py --port 8106
```

Policy pin for the Workshop baseline is `harness=classify`, `config=classify`.
Gold stays private. A scoped Responses policy is loopback-only unless
`SYNTH_RESPONSES_ALLOWED_ENDPOINTS` allowlists the host.

## HealthBench 2 — physician-rubric chat

Zero-generation smoke: train seeds `0,1` and heldout `100,101`, two workers,
`$0.50` ceiling. Workshop recipe `eval.healthbench.smoke.v1`. Policy and
scorer are independent paid roles.

| Role | Path |
| --- | --- |
| Target id | `healthbench_chat` |
| Runtime | `containers/src/synth_containers/platform/runtimes/healthbench.py` |
| Target spec | `containers/src/synth_containers/platform/targets.py` (`HEALTHBENCH_CHAT`) |
| Loopback server | `containers/examples/serve_healthbench.py` (default `127.0.0.1:8114`) |
| Cookbook launcher | `synth-cookbooks-public/cookbooks/optimizers/gepa/healthbench_groq/run_container.sh` |
| Bounded profile | `synth-cookbooks-public/cookbooks/optimizers/gepa/healthbench_groq/eval_smoke.toml` |
| Cookbook contract | `synth-cookbooks-public/cookbooks/optimizers/gepa/healthbench_groq/README.md` |
| Contract tests | `synth-cookbooks-public/cookbooks/optimizers/gepa/healthbench_groq/` → `pytest -q test_container_contract.py` |
| Platform tests | `containers/tests/test_healthbench_platform.py` |
| Visual | `experiment.overview.v1` with separate policy / grader usage lanes |

```bash
cd /Users/joshuapurtell/Documents/GitHub/containers
uv run python examples/serve_healthbench.py --port 8114 \
  --storage-root /tmp/healthbench-workshop-runs
```

Workshop registers policy `openai_gpt41_mini` (`gpt-4.1-mini-2025-04-14`)
before any rollout. Canonical scorer stays container-owned
`gpt-4.1-2025-04-14` via `OPENAI_API_KEY`. Do not treat this smoke as GEPA
uplift.

## Craftax — live env (first-class visual)

Fixture engine for CI; gold ReAct for paid live frames.

| Role | Path |
| --- | --- |
| Fixture target | `craftax_engine` |
| Gold ReAct target | `craftax_react` |
| Runtime | `containers/src/synth_containers/platform/runtimes/craftax.py` |
| Loopback gold server | `containers/examples/serve_craftax_react.py` (default `:8097`) |
| Headless 10-seed | `containers/examples/craftax_ten_seeds.py` |
| GEPA cookbook | `synth-cookbooks-public/cookbooks/optimizers/gepa/crafter_container/` |
| Workshop visual | `visuals/families/first_class_example_containers/live.craftax.v1/` |
| Live-eval prototype | `prototypes/live-evals/craftax/README.md` |

`live.craftax.v1` needs real `frame` events. Scripted `craftax_engine` is the
PR/CI target; `craftax_react` needs gold HTTP + a live planner credential.

## Harbor — first-class fold

Harbor is the only public format fold. Content families (Banking77, Craftax,
HealthBench) are not Harbor wraps.

| Role | Path |
| --- | --- |
| Public fixture | `harbor_public` |
| Docker fold | `harbor_docker` |
| Nested DEO | `deo_nested` |
| Runtime | `containers/src/synth_containers/platform/runtimes/harbor.py` |
| Workshop visual | `visuals/families/first_class_example_containers/live.harbor_eval.v1/` |
| Tests | `containers/tests/test_harbor_docker.py`, `containers/tests/test_after_bind_surface.py` |

Serve from the Containers checkout:

```bash
cd /Users/joshuapurtell/Documents/GitHub/containers
uv run python -c "import uvicorn; from synth_containers.platform import create_compat_app; uvicorn.run(create_compat_app('harbor_public'), host='127.0.0.1', port=8095, log_level='warning')"
```

## dig.bench — agent API / mock dungeon

| Role | Path |
| --- | --- |
| Mock | `digbench_mock` |
| Live relay | `digbench_public` |
| Runtime | `containers/src/synth_containers/platform/runtimes/digbench.py` |
| Workshop visual | `visuals/families/first_class_example_containers/live.digbench.v1/` |
| Tests | `containers/tests/test_digbench_live.py` |

No frames. Do not guess stream URLs; bind the descriptor from prepare.

## Shared quality bar

Engineering contract used by the two public eval cookbooks:

- `synth-cookbooks-public/cookbooks/optimizers/gepa/CONTAINER_ENGINEERING.md`
- `containers/src/synth_containers/platform/README.md`
- Workshop recipes: `apps/synth_desktop/src-tauri/src/optimizers/recipes.rs`
- Workshop eval orchestration: `apps/synth_desktop/src-tauri/src/optimizers/container_eval.rs`

From the Containers repo, the cheap gate for the two CUA containers is:

```bash
cd /Users/joshuapurtell/Documents/GitHub/containers
uv run --with pytest pytest -q tests/test_banking77_platform.py tests/test_healthbench_platform.py tests/test_platform_leftovers.py
```
