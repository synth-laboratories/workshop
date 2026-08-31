# Synth Laguna Responses sidecar

Self-contained OpenResponses/OpenAI Responses server for Synth Desktop on Apple
silicon. The native path loads Laguna directly in-process through the pinned
open `mlx-vlm` Laguna/NVFP4 implementation. It does not require Poolside.app,
`poolside-mlx-sidecar`, a Chat object, or an internal `/v1/chat/completions`
request.

The portable contract is OpenResponses 2.3.0 from 2026-04-24. A reviewed
OpenAI/Codex overlay adds custom tools, namespaces, deferred tool search, MCP,
shell/apply-patch items, lifecycle endpoints, richer SSE events, and Codex model
catalog metadata. Exact schema commits, SHA-256 values, licenses, and generated
overlay types live under `schemas/`.

## Architecture

```text
HTTP / SSE / WebSocket
        │
validated Responses request + typed items
        │
prompt compiler ── immutable ToolBinding table
        │
NativeMlxBackend (mlx-vlm Laguna, one GPU worker, bounded queue)
        │
canonical ModelEvent stream
        │
semantic Responses events + SQLite WAL persistence
```

Tool lowering exists only inside prompt compilation. Every model call is
rehydrated through its exact binding, so custom input remains raw text and
namespaced calls retain their namespace. Unknown or malformed calls fail
explicitly. Grammar-backed custom tools activate their Lark constraint after
tool selection; strict JSON output uses `llguidance` during sampling. Grammar
activation inspects only a bounded generated-token suffix, so its cost does not
grow with a large Codex prompt. After a complete tool envelope, the sampler
keeps a short grace window for an adjacent parallel call and then stops before
Laguna can append terminal prose; the client continuation owns the next turn.

## Run

```bash
uv venv .venv
uv pip install --python .venv/bin/python -e '.[mlx]'

SYNTH_LAGUNA_MODELS_DIR="$HOME/.config/poolside/models" \
.venv/bin/python -m laguna_daemon --backend mlx_lm
```

The standard service tier is `default`. The backend admits one active MLX
generation plus eight queued requests and returns `model_queue_saturated` when
full. At most two large prompt caches remain resident. SSE comments keep long
local prefills alive without consuming semantic sequence numbers.

## Surface

- Portable: `POST /v1/responses`, WebSocket `/v1/responses`,
  `POST /v1/responses/compact`, `GET /v1/models`.
- Lifecycle: retrieve, delete, cancel, input-items pagination, background
  responses, and `POST /v1/responses/input_tokens`.
- State: SQLite WAL, `previous_response_id`, connection-local WebSocket
  continuation for `store=false`, opaque cursors, signed compaction items.
- Observability: `/health` publishes schema pins/capabilities and
  `/v1/synth/responses/telemetry` publishes redacted latency/usage summaries.

## Capability matrix

| Feature | Native Laguna |
| --- | --- |
| Text and separate reasoning | Supported |
| Stream/non-stream semantic object equivalence | Supported |
| Function, custom, namespace, tool-search continuation | Supported |
| MCP/shell/apply-patch lifecycle items | Client-delegated |
| Strict function JSON and custom Lark grammar | Supported |
| JSON object / strict JSON Schema output | Supported |
| Persistence, cancellation, background, compaction | Supported |
| WebSocket sequential/reconnect recovery | Supported |
| Hosted web search / hosted MCP | Explicit capability error |
| Image/file/audio/video input for this text checkpoint | Explicit capability error |

Server-hosted shell and apply-patch execution stay disabled. MCP hosting requires
an independently allowlisted provider; ordinary Codex/Synth tools are returned
to the client for execution.

## Verification

```bash
./scripts/laguna/test.sh
services/laguna-daemon/.venv/bin/python services/laguna-daemon/scripts/check_schemas.py

SYNTH_LAGUNA_API_KEY=... \
services/laguna-daemon/.venv/bin/python \
  services/laguna-daemon/scripts/run_codex_e2e.py
```

The Codex harness is bounded to a disposable workspace and covers text, shell,
apply-patch, MCP echo, and MCP sum with exact call-count assertions. The pinned
OpenResponses suite is run from commit
`cd31bc2060a27ee87a05ec97f49c84027eb6c3ba` with:

```bash
bun run test:compliance --base-url http://127.0.0.1:7340/v1 \
  --api-key openresponses-test \
  --model poolside/Laguna-XS-2.1-NVFP4-mlx
```

The current acceptance run passes all 17 scenarios, Python SDK 2.53.0, and
Node SDK 7.4.0.

## One runtime

This daemon is the only local Laguna runtime. It owns the MLX weights in its
own process, the single GPU admission slot, the prompt caches, and every
residency and telemetry number the Desktop app displays.

There is no second local server, no managed `mlx_lm.server` child, no `:7334`
upstream, and no engine switch. `tests/test_no_second_runtime.py` enforces
this: the production package cannot import a process-spawning module or
reference the legacy port. If you are reading a process list while debugging,
note that `poolside-mlx-sidecar` on `:63300` belongs to Poolside.app — it is
not this daemon, not a dependency, and must not be reused.

`SYNTH_LAGUNA_EXTERNAL_URL` still selects a *remote native Responses* provider,
which the passthrough backend forwards to. That path has no local process
management and cannot serve Chat Completions, because forwarding a Chat request
would mean fabricating a Responses body it was never given.

## Two peer surfaces

`/v1/responses` and `/v1/chat/completions` are peers, not a protocol and its
adapter. Both compile onto the neutral turn core
(`responses_api/compiler.compile_messages`) and execute on the same
`TurnRunner`, so residency, admission, cancellation, and token accounting are
shared rather than duplicated. Neither request is ever lowered into the other
protocol's objects; a Responses `custom` tool never becomes a Chat `function`.

Chat types live only in `chat_api/`. Because Chat cannot faithfully carry the
custom, namespace, MCP, shell, and apply-patch tool kinds, it rejects them with
a stable error rather than degrading them. It is stateless by protocol and owns
no store.

## Inference telemetry

- `GET /v1/synth/inference` — redacted live snapshot: residency, queue depth,
  the active generation's phase and real timings, and bounded rolling
  aggregates fed by both surfaces.
- `GET /v1/synth/inference/stream` — the same payload as SSE, for the Desktop
  monitor.
- `GET /metrics` — Prometheus exposition of the same numbers.
- `POST /v1/synth/model/unload` — explicit residency release; `409` while a
  generation is in flight.

Every metric is measured or `null`. An unmeasured value is never reported as a
zero or an interpolation, and Apple GPU utilization is reported as unavailable
rather than derived from process CPU. Telemetry never carries prompt text,
reasoning text, tool input or output, credentials, file contents, or a complete
response id.

`SYNTH_LAGUNA_IDLE_UNLOAD_SECONDS` defaults to 900. Set it low only to watch
the residency cycle during development.

## Live suite

The deterministic suite never loads weights. The live integration suite runs
against a real daemon and covers both surfaces, tool calling, prompt-cache
reuse, throughput, disconnect handling, and the residency cycle:

```bash
./scripts/laguna/live_test.sh                  # defaults to :7333
```

It skips itself unless `SYNTH_LAGUNA_LIVE_BASE_URL` is set, and writes measured
throughput to `SYNTH_LAGUNA_LIVE_REPORT` so runs stay comparable.
