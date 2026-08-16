# Eval loopback driver — `synth.eval-driver.v1`

Profile-gated programmatic driver surface for live Workshop development
instances. Same category as Visuals IPC: loopback-only bind, bearer token,
descriptor JSON in the instance data root. **Compiled into debug builds only;
never spawned for the canonical installed/production app.**

## Location

| Artifact | Path |
| --- | --- |
| Implementation | `apps/synth_desktop/src-tauri/src/eval_driver.rs` |
| Descriptor | `$SYNTH_DESKTOP_DATA_ROOT/eval-driver.json` |
| Instance pointer | `instance.json` → `evalDriver` (patched at spawn) |

## Activation

Spawned only when **all** of:

1. `cfg!(debug_assertions)` (release/production builds never listen)
2. Named development instance (`SYNTH_DESKTOP_INSTANCE`) **or** explicit
   `SYNTH_DESKTOP_EVAL_DRIVER=1`

## Descriptor

```json
{
  "schemaVersion": "synth.eval-driver.v1",
  "url": "http://127.0.0.1:<ephemeral>",
  "token": "synth_eval_<uuid>",
  "path": ".../eval-driver.json",
  "instanceName": "eval-1",
  "sourceRevision": "<git short>"
}
```

File mode `0600` on Unix. Runners must read this file (or the
`evalDriver.descriptorPath` on the instance manifest) and send:

```http
Authorization: Bearer <token>
X-Synth-Eval-Driver: synth.eval-driver.v1
```

On protocol mismatch the driver responds `426` and refuses the call. Missing
bearer → `401`.

## Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` / `/v1/health` | Liveness + protocol + instance diagnostics |
| `POST` | `/v1/sessions` | Create Codex session (`create_session`) |
| `POST` | `/v1/sessions/{id}/messages` | Send turn (`send_message`) |
| `POST` | `/v1/sessions/{id}/wait_terminal` | Poll journal until terminal run event |
| `GET` | `/v1/sessions/{id}/export` | Journal-backed session export |
| `GET/POST` | `/v1/containers…` | Register / list / probe (shared with visuals IPC) |
| `POST` | `/v1/containers/{id}/rollouts` | Scripted transport-gate rollouts |
| `POST` | `/v1/containers/{id}/policy_rollouts` | Class-A LLM policy rollouts (`ProviderClass` dispatch) |
| `POST` | `/v1/policy_preflight` | Fail-closed credential/daemon check before a paid batch |
| `POST` | `/v1/laguna/ensure` | Ensure the product-managed Laguna runtime and return its status receipt |
| `GET` | `/v1/laguna/status` | Read the same refreshed Laguna status shown by the UI |
| `GET` | `/v1/laguna/inference` | Read daemon-backed model residency and inference telemetry |
| `POST` | `/v1/laguna/model/unload` | Request model unload through the production Laguna manager |
| `POST` | `/v1/open_visual` | Create + show a visual (dogfood only; never grade from it) |
| `POST` | `/v1/visuals/{id}/update` | Patch visual bindings/metadata (live → sealed digests) |
| `POST` | `/v1/traces/ingest` | Generic Trace V5 import via `inventory.ingest_trace_bundle` |

Aliases (`/v1/create_session`, `/v1/send_message`, `/v1/wait_for_terminal`,
`/v1/export_session`, `/v1/container_register`, `/v1/container_probe`) mirror the
semantic action names used by `window.__synthEval`.

## Policy rollout body

```json
{
  "taskInstanceId": "craftax:test:2001",
  "policyRef": { "harness": "react", "config": "luna_med" },
  "provider": "openrouter",
  "model": "openai/gpt-5.6-luna",
  "reasoningEffort": "medium",
  "timeoutS": 600,
  "telemetry": {
    "enabled": true,
    "transport": "sse",
    "detail": "standard",
    "frame": { "enabled": false }
  }
}
```

`policy_ref` is required (`harness` and `config`; `config` is omitted only for
`isolated_policy_process`). The driver does not pick a harness or default
`luna_med`. The coding agent via MCP supplies the pin. The container runs the
policy after the visual is bound to the declared SSE URL and `stream.subscribed`
is observed. Workshop does not call the model or `/step`.

`provider` on `POST /v1/policy_preflight` is one of `openrouter` |
`synth-cloud` | `local-laguna` and resolves through the same
`codex::provider_class` table as Codex session preparation:

| provider | credential / daemon | chat endpoint |
| --- | --- | --- |
| `openrouter` | host OpenRouter key | OpenRouter `/chat/completions` |
| `synth-cloud` | brokered Synth API key | `{backend}/api/v1/responses` |
| `local-laguna` | `LagunaManager.ensure` + local key | `{laguna}/v1/chat/completions` |

Call `POST /v1/policy_preflight` before a multi-rollout batch. Missing keys or
an unavailable Laguna daemon fail closed before any paid call. Provider secrets
never leave the host process and must never appear in case files, compose env,
results, or traces.

The Laguna lifecycle endpoints are authenticated debug-driver projections of
the production `LagunaManager`; they do not contact a test daemon or synthesize
readiness. `ensure` also performs the daemon's idempotent production model-load
operation, matching the pre-turn path used to restore residency after unload.
Local Codex session preparation then reads the daemon's authenticated
`/v1/models` native Codex envelope and materializes only the exact configured
slug. Instructions, reasoning levels, service tiers, modalities, and tool
capabilities are validated and retained as one daemon-owned unit; Workshop does
not clone an arbitrary bundled Codex model as a local fallback.
It returns `{ok, baseUrl, status}` so a runner can distinguish
an available runtime from an explicit `not_installed`, `unavailable`, or
`error` prerequisite. The nested `status` retains the UI's product status fields
and adds stable `outcome` / `code` fields. Runners may skip only
`unmet_prerequisite` with `weights_unavailable` or `hardware_unsupported`;
`runtime_unavailable` is a product error. `status`, `inference`, and
`model/unload` otherwise return the same redacted DTOs used by the UI. Unload
keeps the production `{released, conflict, detail}` semantics. The daemon API
key remains in the host process.

Successful policy rollouts also return `traceCorrelation` using
`synth.trace-correlation.v1`. It is **correlation proof**, not a Trace V5
substitute: one actual action's observation, action, reward, immutable frame,
and provider response share the same rollout identity and environment step.
Durable Trace V5 records are sealed by the eval runner (`synth-trace`) and
imported through `POST /v1/traces/ingest`. `frame.sha256` hashes the bytes at
`frame.url`; `modelEvent.id` is the durable Workshop journal event and
`modelEvent.providerResponseId` is the real provider response id.
`modelEvent.boundRolloutId` must equal the top-level `rolloutId`. The driver
fails the request rather than emitting a partial or synthetic correlation.

### Trace ingest body

```json
{
  "sourcePath": "/abs/path/to/trace.zip-or-dir",
  "sourceKind": "synth-trace-v5",
  "title": "optional title",
  "sourceUri": "optional://uri"
}
```

Returns the inventory `TraceBundleIngestResult` (`trusted`, digests, traces).

## Cross-repo contract

The **evals** repo owns cases, runner, graders, compose, and results under
`evals/workshop/`. It vendors these protocol types and asserts
`synth.eval-driver.v1` at connect. No package imports across the two repos.
Each `run-manifest.json` records the Workshop `sourceRevision` it drove.
Health instance diagnostics also expose the launcher's validated
`executableDigest` when it is a qualified SHA-256. If a development launcher
starts before the binary exists, the running app hashes its own executable as
the provenance fallback. Session exports include a
top-level, typed `providerBinding` with provider, model, endpoint class,
credential-binding class, brokered status, and `fallbackAllowed: false`; raw
endpoints and credentials are deliberately excluded.
