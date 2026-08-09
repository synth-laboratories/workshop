# Native Responses server for MLX/Laguna

Status: proposed architecture and implementation plan  
Date: 2026-08-09  
Owner: `services/laguna-daemon`

## Decision

Replace the current Responses-to-Chat compatibility path with a spec-first
Responses server whose protocol model is independent of its inference backend.
The server will use a native MLX backend on Apple Silicon and may use explicit
backend adapters for development or remote inference, but Chat Completions will
not be its canonical representation.

The compatibility target has two layers:

1. **OpenResponses 2026-04-24 core** for portable items, semantic streaming,
   function tools, continuation, compaction, and WebSocket behavior.
2. **OpenAI/Codex extension profile** for custom tools, namespaces and deferred
   tool search, MCP items, shell/apply-patch items, additional lifecycle
   endpoints, and the event variants Codex app-server actually consumes.

The extension profile is required for Synth Desktop. Passing only the portable
OpenResponses suite is not sufficient to claim Codex compatibility.

## Why this is necessary

- Installed `mlx-lm` 0.31.3 exposes `/v1/chat/completions` and
  `/v1/completions`, but no `/v1/responses` route.
- The live Poolside-compatible local server on `127.0.0.1:63300` also returns
  `404` for `/v1/responses`.
- The open MLX-LM Responses pull request (#1207) is not merged. Its stated
  design translates Responses into the Chat pipeline and filters unsupported
  tool types, so it is not a complete Codex contract.
- The existing Laguna adapter loses information that Chat cannot represent.
  In particular, a Responses `custom` tool is lowered to a Chat `function`,
  then reconstructed as `function_call`; Codex rejects the result instead of
  dispatching the MCP bridge.
- OpenResponses defines item state machines and semantic event ordering. A
  correct server cannot be implemented as ad hoc JSON renaming at the HTTP
  boundary.

## Product requirements

The server must:

- run locally on Apple Silicon with Laguna XS MLX weights;
- work with Codex app-server using `wire_api = "responses"`;
- preserve every item and tool kind without a lossy Chat round trip;
- support streaming and non-streaming responses with identical final objects;
- support multiple concurrent Desktop instances safely;
- expose honest model capabilities and reject unsupported behavior explicitly;
- retain no user content when `store=false`, except bounded connection-local
  continuation state required by an active WebSocket;
- provide deterministic protocol tests without loading a 20 GB model;
- provide live MLX and Synth Desktop acceptance tests before replacing the
  current daemon.

It must never silently discard an input modality, tool type, reasoning item,
format constraint, or continuation reference.

## Architecture

```text
HTTP/SSE/WebSocket
        |
        v
request validation + capability negotiation
        |
        v
Responses turn coordinator
  - item/context graph
  - response/item state machines
  - cancellation and deadlines
  - persistence / previous_response_id
        |
        v
prompt compiler <---- tool registry + structured-output compiler
        |
        v
ModelBackend protocol
  +-- NativeMlxBackend (production local path)
  +-- FakeBackend (protocol and failure tests)
  +-- RemoteResponsesBackend (native passthrough only)
        |
        v
canonical ModelEvent stream
        |
        v
Responses event assembler
  - IDs, indices, sequence numbers
  - item/content lifecycles
  - usage and terminal response
```

The API, orchestration, storage, and MLX sampling layers are separate. The
event assembler is the only component allowed to serialize Responses SSE.

### Proposed package layout

```text
laguna_daemon/
  responses_api/
    models/                 # generated core types + reviewed extensions
    validation.py           # semantic validation beyond JSON Schema
    capabilities.py         # server and per-model capability profiles
    ids.py                   # typed resp_/msg_/fc_/ctc_/call_ IDs
    state.py                 # response and item state machines
    events.py                # canonical events and SSE/WebSocket encoding
    coordinator.py           # one response turn
    context.py               # item graph and prompt-context assembly
    storage.py               # ResponseStore protocol
    sqlite_store.py          # default WAL store and migrations
    compaction.py
    tools/
      registry.py
      function.py
      custom.py
      namespace.py
      hosted_mcp.py
      shell.py
      apply_patch.py
    backends/
      protocol.py
      mlx.py
      fake.py
      remote_responses.py
  routes/
    responses.py
    responses_websocket.py
```

The current `responses.py` translator becomes a temporary legacy module and is
deleted after the native MLX and Codex gates are green.

## Contract and schema strategy

Pin the exact OpenResponses release and record its source hash. Vendor its
Apache-2.0 OpenAPI document under `schemas/openresponses/2026-04-24/`, retain
the required notices, and generate Pydantic models. Do not hand-maintain the
portable union types.

Maintain a small reviewed overlay for the OpenAI/Codex extension profile,
derived from the current official OpenAI OpenAPI schema. The overlay must add,
at minimum:

- `custom` tool definitions, `custom_tool_call`, and
  `custom_tool_call_output`;
- `response.custom_tool_call_input.delta` and `.done`;
- namespace tools and client/server deferred `tool_search`;
- MCP list/call/approval item families and stream events;
- shell, apply-patch, local-shell, and their output items;
- the additional response lifecycle endpoints used by supported clients.

Generation must be reproducible. CI fails when regenerated types or the pinned
schema hash change. Unknown request fields are rejected by default; explicitly
documented provider extensions live under a namespaced extension field.

## Canonical item model

The coordinator operates on typed items, not Chat messages. It preserves the
input order exactly and supports:

- user, system, developer, and assistant messages;
- text, refusal, image, file, video, and future namespaced content parts;
- reasoning and compaction items, including opaque encrypted content;
- function calls and outputs;
- custom tool calls and outputs with raw `input`, never JSON-coerced;
- namespace and tool-search call/output items;
- MCP list, approval, call, result, and error items;
- shell/apply-patch calls and outputs;
- item references when persistent state is enabled.

Every item has a validated lifecycle. Terminal items are immutable. An
`incomplete` item must be the final output item and makes its response
`incomplete`. IDs use the correct family prefix and `call_id` is distinct from
the output item ID.

## Native MLX backend

`NativeMlxBackend` owns model loading, prompt caches, admission control, and
sampling. It implements a narrow internal protocol:

```python
class ModelBackend(Protocol):
    async def capabilities(self, model: str) -> ModelCapabilities: ...
    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate: ...
    async def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]: ...
    async def cancel(self, generation_id: str) -> None: ...
```

`ModelEvent` variants include text, reasoning, refusal, function arguments,
custom input, tool selection, usage, finish, and backend error. They contain no
HTTP or Chat-Completions fields.

The first implementation may reuse reviewed MLX-LM generation primitives and
its tokenizer tool parser, but it must not proxy through
`/v1/chat/completions`. Any use of private MLX-LM APIs is isolated behind this
backend and pinned to a tested MLX-LM version. A compatibility test runs against
the oldest and newest supported versions.

### Prompt compilation and tool rehydration

Models still need a chat template internally. That is a prompt-compilation
detail, not the API model.

For each request, the compiler creates an immutable `ToolBinding` table:

```text
model-visible name -> original tool kind, namespace, schema/grammar, caller,
                      output item type, and authorization policy
```

If a tokenizer can only represent function-shaped tools, custom and namespaced
tools may be lowered only inside the compiled prompt. Parsed model output is
rehydrated through the exact binding table before it becomes a `ModelEvent`.
Unknown, ambiguous, or malformed calls fail explicitly; they never default to
`function_call`.

Strict function tools validate arguments against their JSON Schema. Custom
tools preserve raw text and enforce their declared text or grammar format.
Structured output constraints and tool constraints are compiled together and
must fail before sampling if the backend cannot enforce them.

## Streaming correctness

The event assembler assigns one monotonically increasing `sequence_number` per
response and emits the complete semantic lifecycle:

1. `response.created`, then `response.in_progress` when applicable;
2. `response.output_item.added` before any delta for that item;
3. content-part added/delta/done events for message content;
4. function argument or custom input delta/done events for tool calls;
5. `response.output_item.done` with the final immutable item;
6. exactly one terminal `response.completed`, `response.incomplete`, or
   `response.failed` event;
7. the literal SSE `data: [DONE]` terminator.

Every delta includes `item_id`, `output_index`, and required content indices.
The final streamed response must deep-equal the non-streaming response after
excluding timestamps and generated IDs. Disconnect and cancellation close the
backend generation and never publish a false completed state.

## State, continuation, and concurrency

Implement a `ResponseStore` abstraction with SQLite WAL as the local default.
All mutations go through one async writer queue, while reads use independent
connections. This is sufficient for multiple Desktop instances because there
is one Laguna daemon writer process; libSQL/Turso is not required for V1.

Persist, when `store=true`:

- the validated request and effective configuration;
- ordered input and output items;
- response status, usage, errors, and timestamps;
- parent linkage and prompt-cache identity;
- cancellation/background state.

`previous_response_id` reconstructs context in the required order:

```text
previous input -> previous output -> new input
```

For `store=false`, HTTP clients must resend history. An active WebSocket keeps
only bounded connection-local continuation state. Failed continuation evicts
that state as required by OpenResponses. A socket processes one response at a
time and supports sequential turns, a 60-minute connection limit, and clean
recovery errors.

Use context budgets based on the model's real tokenizer. `truncation=disabled`
returns a pre-sampling error. `truncation=auto` drops only complete eligible
items and records what was removed. Tool call/output pairs and required
reasoning state are atomic during truncation.

## Endpoint surface

### Required portable core

- `POST /v1/responses`
- WebSocket upgrade on `/v1/responses`
- `POST /v1/responses/compact`
- `GET /v1/models`

### OpenAI-compatible lifecycle extension

- `GET /v1/responses/{response_id}`
- `DELETE /v1/responses/{response_id}`
- `POST /v1/responses/{response_id}/cancel`
- `GET /v1/responses/{response_id}/input_items`
- `POST /v1/responses/input_tokens`

Background responses use a bounded queue and explicit queued/in-progress/
terminal states. Cancellation is idempotent. Pagination uses stable item order
and opaque cursors.

## Reasoning, formats, and modalities

- Preserve Laguna reasoning as a `reasoning` item, with separate summary and
  content streams. Never mix hidden reasoning into assistant output text.
- Treat encrypted reasoning as opaque round-trippable data.
- Implement `text.format` for plain text, JSON object, and strict JSON Schema.
  Invalid structured output produces an incomplete/model error according to
  the selected policy; it is not returned as valid JSON.
- Implement input token counting through the same prompt compiler used for
  sampling.
- Decode and validate image/file/video content behind a `ContentResolver` with
  size, MIME, URL, redirect, and timeout limits.
- Capability negotiation is per model. Text-only Laguna returns a precise 400
  for vision/audio/video requests. The server architecture supports an
  `mlx-vlm` backend later; it must never pretend a text-only model consumed an
  image.

“Full API support” means every defined feature is validated and either executed
correctly or rejected with a stable, documented capability error. It does not
mean fabricating unsupported model behavior.

## Hosted tools and safety

Developer-hosted function/custom calls yield control to Codex or the client.
Server-hosted tools run only through explicit, independently testable
executors:

- MCP client with allowlisted transports, DNS/IP egress policy, OAuth/token
  isolation, timeouts, output limits, and approval flow;
- shell/apply-patch executors disabled by default in the inference daemon and
  delegated to the client for Synth Desktop;
- optional web/file/code-interpreter providers behind capability flags.

Tool output is untrusted data. It is size-limited, tagged with provenance, and
never interpolated into system instructions. Approval IDs are single-use and
bound to response, tool, arguments, and principal.

## Errors and observability

Use one typed error taxonomy across HTTP, SSE, and WebSocket:

- invalid request/schema/parameter;
- unsupported model capability;
- model or tokenizer failure;
- context length and output budget exhaustion;
- previous response not found;
- tool parse, validation, approval, execution, and timeout failures;
- cancellation, overload, and internal errors.

Return stable codes, the relevant parameter, safe messages, and correct HTTP
status. Streaming errors are followed by `response.failed` and `[DONE]`.

Structured telemetry includes response ID, model, backend, queue/load/sample
latency, token counts, cache hits, item/event counts, finish reason, and error
code. Never log bearer tokens, tool credentials, raw custom-tool input, file
contents, or prompts by default. A redacted debug trace can be enabled per
request in development.

## Verification strategy

### Schema and state-machine tests

- validate generated types against pinned OpenAPI examples;
- property-test valid and invalid item transitions;
- fuzz polymorphic input unions and unknown fields;
- assert typed ID families, stable ordering, and call/output pairing;
- assert non-stream and reconstructed stream equivalence;
- snapshot every supported event sequence and every failure sequence.

### Backend contract tests

Run the same suite against `FakeBackend` and `NativeMlxBackend`:

- text, reasoning, refusal, stop, max tokens, and cancellation;
- parallel and sequential tool calls;
- strict function JSON, custom freeform input, and grammar-constrained input;
- malformed/unknown tool calls and interrupted tool arguments;
- structured output and real tokenizer counts;
- concurrent requests, queue saturation, disconnects, and model unload.

### External compliance

- pin and run all 17 OpenResponses HTTP/WebSocket acceptance scenarios;
- run official OpenAI Python and JavaScript SDK smoke suites;
- validate every response and event against the pinned schemas;
- differential-test portable requests against a known native Responses server,
  allowing only documented provider differences.

### Codex/Synth acceptance

Capture real Codex app-server request fixtures and require:

1. ordinary function call and output continuation;
2. `custom_tool_call` with raw input and correct custom output continuation;
3. namespace/deferred tool-search discovery;
4. shell and apply-patch round trips in an isolated temporary workspace;
5. MCP aggregate bridge dispatch without `unsupported call`;
6. live `synth_containers` discovery of Craftax on `127.0.0.1:8098`;
7. exactly two live rollout IDs returned;
8. rollout and evaluation visuals created and opened in Synth Desktop;
9. the same agent loop under streaming, reconnect, and restored Desktop session.

The canonical installed app is replaced only when the full standard gate and
the live Codex/Synth acceptance gate are green.

## Delivery phases

### Phase 0 — contract capture and guardrails (2-3 days)

- Pin OpenResponses and OpenAI schema snapshots and licenses.
- Capture sanitized Codex request/response fixtures, including the failing MCP
  custom tool call.
- Add schema generation/check commands and a capability matrix.
- Freeze the current adapter behind `SYNTH_LAGUNA_RESPONSES_ENGINE=legacy`.

Gate: generated schemas are reproducible; fixtures fail for the known reason.

### Phase 1 — protocol core with fake backend (4-6 days)

- Implement typed requests/items/responses, IDs, validation, coordinator,
  state machines, errors, SSE assembler, and non-stream/stream equivalence.
- Add SQLite store, continuation, retrieval/deletion, token-count shell, and
  cancellation primitives.

Gate: core HTTP OpenResponses tests pass without MLX.

### Phase 2 — native MLX text and reasoning (4-6 days)

- Implement `NativeMlxBackend`, prompt compiler, token counting, cancellation,
  usage, prompt-cache integration, and admission control.
- Remove the internal HTTP hop to Chat Completions for the native path.

Gate: real Laguna text/reasoning/structured-output tests pass under load.

### Phase 3 — Codex tools (4-7 days)

- Implement function, custom, namespace/tool-search, output continuation, and
  exact tool-binding rehydration.
- Add required shell/apply-patch item codecs and Codex event variants.
- Run the Craftax two-rollout visual acceptance flow.

Gate: Codex completes the live container task without shell fallback or an
unsupported tool call.

### Phase 4 — persistence, WebSocket, compaction, background (5-7 days)

- Complete previous-response persistence and pagination.
- Implement all WebSocket continuation/recovery semantics.
- Implement compaction, background queue, cancel, and input-token endpoints.

Gate: all 17 OpenResponses compliance scenarios pass.

### Phase 5 — extended modalities and hosted tools (separate tracks)

- Add `mlx-vlm`/content resolver support for vision-capable models.
- Add hosted MCP and optional web/file/code-interpreter providers.
- Expand the OpenAI extension conformance matrix as features become enabled.

Gate: each capability is enabled only when its schema, security, failure, and
live tests are green.

### Phase 6 — cutover and deletion (2-3 days)

- Make `native` the default, retain `legacy` for one release as rollback.
- Add health fields for engine, schema versions, capabilities, and conformance.
- Soak with multiple isolated Synth Desktop instances.
- Delete the legacy translator after the rollback window.

## Estimated effort

- Codex-quality local text + custom/function tooling: roughly 2-3 engineer
  weeks including tests and live acceptance.
- Full OpenResponses HTTP/WebSocket/compaction compliance: roughly 4-5 weeks.
- Broad OpenAI hosted-tool and multimodal parity: additional independent work;
  it should not block the local Laguna/Codex cutover.

These estimates assume one experienced engineer and reuse of MLX-LM's model,
tokenizer, tool-parser, and cache primitives behind a pinned backend adapter.

## Immediate next implementation slice

Start with Phase 0, then build Phase 1 around the exact failing custom-tool
fixture. Do not add more branches to the current Chat translator. The first
vertical slice is:

```text
Codex Responses request with custom mcp__synth_containers tool
  -> validated typed request
  -> fake backend emits CustomInput events
  -> exact custom_tool_call SSE lifecycle
  -> custom_tool_call_output continuation
  -> completed response
```

Once that slice is protocol-correct, substitute `NativeMlxBackend` and run the
same contract before returning to the Craftax UI acceptance test.
