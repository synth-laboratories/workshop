# Handoff: Workshop secrets proxy → optimizer integration

**Status:** Workshop side ready to consume  
**Worktree:** `/Users/joshuapurtell/Documents/Codex/2026-08-18/e/work/workshop-v06`

Do not restore a raw-key, process-environment, `.env`, or `secrets.toml` fallback.

The missing cross-repository piece was a trusted container route in the eval worker contract. Without it, Craftax retained the recipe route and attempted to send the Workshop sentinel to `api.openai.com`.

## What Workshop issues for a paid eval worker

Host process (`python -m synth_optimizers.eval worker`):

| Variable | Value |
| --- | --- |
| `OPENAI_API_KEY` | `workshop-proxy` sentinel; not the capability |
| `OPENAI_BASE_URL` | `http://127.0.0.1:<port>/cap/<wcap_…>/v1/providers/openai` |
| `WORKSHOP_CAPABILITY` | `wcap_…` |
| `WORKSHOP_CAPABILITY_FILE` | Owner-only (`0600`) file containing the handle |
| `WORKSHOP_OPENAI_ROUTE` | Container-reachable Chat Completions URL |
| `WORKSHOP_OPENAI_BASE_URL` | Container-reachable SDK base URL |

Workshop patches `eval.worker-manifest.v1` before spawn:

```json
{
  "credential_mode": "workshop_proxy",
  "provider_routes": {
    "openai": "http://host.docker.internal:<port>/cap/<wcap_…>/v1/providers/openai/chat/completions",
    "openai_base": "http://host.docker.internal:<port>/cap/<wcap_…>/v1/providers/openai",
    "auth": "capability_path",
    "api_key_sentinel": "workshop-proxy",
    "container_host": "host.docker.internal",
    "extra_hosts": ["host.docker.internal:host-gateway"]
  }
}
```

Authentication is carried by the `/cap/wcap_…/` path. A container may send `Authorization: Bearer workshop-proxy`. It must not receive the capability as `OPENAI_API_KEY`, and it must not call `https://api.openai.com`.

The proxy stays bound to loopback and is not exposed to the LAN. Docker Desktop on macOS reaches it through `host.docker.internal`. Linux needs `host.docker.internal:host-gateway` in `extra_hosts`. The host may be overridden with `WORKSHOP_PROXY_CONTAINER_HOST`.

The capability is scoped to:

- run ID;
- recipe ID;
- provider;
- recipe model allowlist;
- `chat.completions.create`;
- spend and call ceilings;
- TTL.

It is revoked when the run drops.

Paid-eval failures use `contract: "workshop.secrets_proxy"` and one of:

- `missing_credential`;
- `secrets_proxy_unavailable`;
- `secrets_proxy_denied`;
- `secrets_proxy_route_unbound`;
- `secrets_proxy_unreachable`.

## Required `synth-optimizers` and Craftax changes

1. Read `manifest.provider_routes.openai`, with `WORKSHOP_OPENAI_ROUTE` as the trusted host-provided representation where needed. Candidate and recipe inputs must not override it.
2. Forward the trusted URL into the trial container as `EVAL_LLM_ROUTE`. Set `OPENAI_BASE_URL` to `provider_routes.openai_base` when an SDK requires a base URL.
3. Set container `OPENAI_API_KEY=workshop-proxy`. Never copy `WORKSHOP_CAPABILITY` into `OPENAI_API_KEY`.
4. Preserve the recipe's model, effort, pricing, and budget allowlists. Replace only the provider route.
5. Add Docker/Podman host mapping `host.docker.internal:host-gateway` where required.
6. Fail closed before starting a paid trial if `provider_routes` is absent, still contains `127.0.0.1` or `localhost`, or contains `api.openai.com`.

## Acceptance

Rerun `eval.craftax.llm-policy.smoke.v1` and prove:

- container environment contains the sentinel and proxy route only;
- no raw key appears in arguments, environment dumps, manifests, events, logs, or artifacts;
- every model request reaches the Workshop proxy and none goes directly to OpenAI;
- proxy audit records run, recipe, model, and usage attribution;
- cancel and terminal completion revoke the capability;
- replaying the capability after revocation returns `401`;
- the run emits real trial events rather than terminating with zero rollouts.

Previous failed comparison point:

- Run: `opt_eval_d2ea1c28916b`
- Visual: `vis_4581a9f35d5c4bef86b268da4be1d765`
- Failure: the worker could not resolve `OPENAI_API_KEY` because the recipe route still pointed at OpenAI.

## Ownership boundary

Workshop-side binding is complete: paid eval workers receive a recipe-scoped capability; the manifest carries `provider_routes.openai` on `host.docker.internal`; and `OPENAI_API_KEY` remains a sentinel.

The remaining optimizer-side responsibility is to forward that trusted route into the Craftax container instead of using the recipe's direct OpenAI URL.
