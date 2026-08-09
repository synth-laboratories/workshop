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
tool selection; strict JSON output uses `llguidance` during sampling.

## Run

```bash
uv venv .venv
uv pip install --python .venv/bin/python -e '.[mlx]'

SYNTH_LAGUNA_RESPONSES_ENGINE=native \
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
OpenResponses compliance command is documented in
`RESPONSES_SERVER_PLAN.md`; the current acceptance run passes all 17 scenarios,
Python SDK 2.53.0, and Node SDK 7.4.0.

## Rollback window

`SYNTH_LAGUNA_RESPONSES_ENGINE=legacy` temporarily restores the reviewed
Chat-translation adapter during the cutover window. Native is the default. The
legacy flag is not a second architecture and should be removed after the
installed Desktop soak described in `RESPONSES_SERVER_PLAN.md`.
