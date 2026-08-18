---
name: use-synth-secrets
description: Use when listing Workshop provider connections, importing a .env into the local vault, or requesting bounded use of a registered credential. Never read or paste secret values.
---

# Use Synth Secrets

Codex advertises one compact custom tool, `mcp__synth_secrets__secrets_manage`. In code mode call it as `tools.mcp__synth_secrets__secrets_manage({ operation, ... })`.

The vault is **local Workshop**, on this device. Values live in the OS credential store. Tool results are aliases, status, and masked suffixes (`••••7F2A`) only.

## Operations

| `operation` | Arguments | Result |
| --- | --- | --- |
| `list` | `provider?`, `scope?` | Registered connections. Empty means none yet. |
| `request_env_import` | `sourcePath` (absolute), `variableNames?` | Host reads the file. You get names + masked suffixes. The user must approve in **Settings → Secrets**. |
| `request_use` | `secretId`, `runId?`, `recipeId?` | `approval_required` until the user allows it in Settings, then retry to receive a `wcap_…` handle for the local proxy. |

## Do not

- `cat` a `.env`, `.env.*`, or `secrets.toml`. Those paths are plaintext credential stores. Codex sandbox cannot deny those reads; this skill and `AGENTS.md` are the policy. Use `request_env_import` even if you already know they exist.
- Pass `value`, `apiKey`, `token`, `password`, or the secret itself as an argument.
- Call a create / reveal / export / commit / get tool. Those do not exist.
- Put provider keys in command-line arguments, eval `secrets.toml`, or child environments. Workloads use `OPENAI_API_KEY=workshop-proxy`, a capability-prefixed `OPENAI_BASE_URL` on the host, and `WORKSHOP_OPENAI_ROUTE` (`host.docker.internal`, never `api.openai.com`) inside containers.

## Import a workspace `.env`

1. `list` first.
2. `request_env_import` with the **absolute** path, for example `/Users/you/proj/.env`.
3. Tell the user to open **Settings → Secrets**, review the masked candidates, and import.
4. `list` again after they approve.

## After use is granted

The handle is not the provider key. If a handle is returned, it is only a Bearer token for Workshop's loopback provider proxy. Do not log it, put it in git, or treat it as an API key.

## Optimizer container integration

For the authoritative paid-eval worker and container route contract, failure
codes, and Craftax acceptance test, see
`../../../../docs/HANDOFF_SECRETS_PROXY_OPTIMIZER_ROUTE_2026-08-18.md`.
