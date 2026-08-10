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
| `POST` | `/v1/containers/{id}/policy_rollouts` | Class-A LLM policy rollouts (OpenRouter key from host `synth_config`) |
| `POST` | `/v1/open_visual` | Create + show a visual (dogfood only; never grade from it) |

Aliases (`/v1/create_session`, `/v1/send_message`, `/v1/wait_for_terminal`,
`/v1/export_session`, `/v1/container_register`, `/v1/container_probe`) mirror the
semantic action names used by `window.__synthEval`.

## Policy rollout body

```json
{
  "taskInstanceId": "craftax:test:2001",
  "provider": "openrouter",
  "model": "openai/gpt-5.6-luna",
  "reasoningEffort": "low",
  "maxSteps": 64,
  "maxCalls": 16,
  "timeoutS": 600,
  "telemetry": {
    "enabled": true,
    "transport": "sse",
    "detail": "standard",
    "frame": { "enabled": false }
  }
}
```

Scores and achievements come from the container episode record. The OpenRouter
key never leaves the host process and must never appear in case files, compose
env, results, or traces.

Successful policy rollouts also return `traceCorrelation` using
`synth.trace-correlation.v1`. It is a fail-closed proof for one actual action:
the final observation, action, reward, immutable frame, and OpenRouter response
all carry the same rollout identity and environment step. `frame.sha256` hashes
the bytes at `frame.url`; `modelEvent.id` is the durable Workshop journal event
and `modelEvent.providerResponseId` is the real provider response id.
`modelEvent.boundRolloutId` must equal the top-level `rolloutId`. The driver fails
the request rather than emitting a partial or synthetic correlation.

## Cross-repo contract

The **evals** repo owns cases, runner, graders, compose, and results under
`evals/workshop/`. It vendors these protocol types and asserts
`synth.eval-driver.v1` at connect. No package imports across the two repos.
Each `run-manifest.json` records the Workshop `sourceRevision` it drove.
