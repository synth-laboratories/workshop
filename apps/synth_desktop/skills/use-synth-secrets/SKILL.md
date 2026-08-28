---
name: use-synth-secrets
description: Use when locating, registering, or requesting bounded use of a Workshop provider credential. Never read or paste secret values or pass absolute paths.
---

# Use Synth Secrets

Codex advertises one compact custom tool, `mcp__synth_secrets__secrets_manage`. In code mode call it as `tools.mcp__synth_secrets__secrets_manage({ operation, ... })`.

Workshop stores remembered locations and source licenses in SQLite. Credential bytes are loaded from the selected `.env` into process memory and served only through the provider proxy. Tool results never contain values, canonical paths, or masked suffixes.

## Operations

| `operation` | Arguments | Result |
| --- | --- | --- |
| `workspace_roots_list` | none | Opaque approved root references and safe display names. |
| `bindings_list` | `provider?` | Registered source licenses and loaded/preferred state. |
| `locators_list` | `provider?` | Remembered workspace-relative locations. External picker locations are intentionally omitted. |
| `locator_request` | `workspaceRootRef`, `relativePath`, `provider`, `variable`, `label?` | Blocking Remember-location approval. It stats but does not read the file. |
| `locator_status` | `locatorId` | Current location state. |
| `locator_remove` | `locatorId` | Forget the location; the file is not deleted. |
| `source_request` | `locatorId`, `provider`, `variable`, `label?`, or the same workspace-relative fields as `locator_request` | Blocking Register card. The operator may choose Remember only or Register. |
| `source_status` | `locatorId` | Registration, preferred, and loaded state. |
| `source_remove` | `locatorId` | Unload and remove the source license; the remembered location remains. |
| `request_use` | exactly one of `locatorId`, `sourceId`, `secretId`; `runId?`, `recipeId?`, `workload?` | Blocking IssueLease card, then a bounded `wcap_…` handle for the local proxy. |
| `list` | `provider?` | Temporary compatibility alias for `bindings_list`; no suffixes. |
| `request_env_import` | none | Compatibility refusal `credential_locator_compat_import`. |

## Do not

- `cat` a `.env`, `.env.*`, or `secrets.toml`. Use the locator flow even if you already know the file exists.
- Pass `value`, `apiKey`, `token`, `password`, or the secret itself as an argument.
- Pass an absolute path. Start with `workspace_roots_list`, then send its opaque reference plus a relative path.
- Call a create / reveal / export / commit / get tool. Those do not exist.
- Put provider keys in command-line arguments, eval `secrets.toml`, or child environments. Workloads use `OPENAI_API_KEY=workshop-proxy`, a capability-prefixed `OPENAI_BASE_URL` on the host, and `WORKSHOP_OPENAI_ROUTE` (`host.docker.internal`, never `api.openai.com`) inside containers.

## Register a workspace `.env`

1. Call `workspace_roots_list`.
2. Call `bindings_list` and `locators_list`.
3. Call `source_request` with `workspaceRootRef`, a relative path such as `.env`, the provider, and the exact variable name.
4. Wait for the native card to settle. Do not ask the operator to visit Settings or type an approval.
5. If the result is `remembered`, the operator chose Remember only; do not claim the source is loaded.
6. Call `request_use` only after the source reports loaded.

## After use is granted

The handle is not the provider key. It is only a bearer capability for Workshop's loopback provider proxy. Do not log it, put it in git, or treat it as an API key.

## Optimizer container integration

For the authoritative paid-eval worker and container route contract, failure
codes, and Craftax acceptance test, see
`../../../../docs/HANDOFF_SECRETS_PROXY_OPTIMIZER_ROUTE_2026-08-18.md`.
