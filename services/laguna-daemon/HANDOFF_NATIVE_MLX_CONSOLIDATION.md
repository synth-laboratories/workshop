# Handoff: finish the native MLX Responses sidecar and Desktop cutover

Date: 2026-08-09  
Audience: engineer taking over Laguna/Codex integration  
Primary design: [`RESPONSES_SERVER_PLAN.md`](./RESPONSES_SERVER_PLAN.md)  
Status: native protocol core is substantially implemented and its deterministic
tests are green. The one-sidecar deletion pass, current-source live MLX/Codex
gate, inference monitor, and the detached-session UI fix remain.

> ## Update — 2026-08-09, later session
>
> Phases A–E are implemented and their deterministic suites are green. What
> changed relative to the plan below, and what is still open:
>
> **Architecture is now peer surfaces over a neutral core, not a Chat adapter.**
> Phase B below proposed implementing Chat by constructing a typed
> `ResponseRequest` and calling `ResponsesService`. That was not built, because
> the neutral seam already existed: `compile_turn` did exactly one
> Responses-specific thing (`items_to_messages`), and everything after it
> operated on a plain message list. So `compile_messages` was extracted as the
> shared core and `TurnRunner` (`responses_api/runner.py`) now owns the neutral
> execution — admission registry, cancel propagation, the `aclosing` slot
> release, and telemetry. `/v1/responses` and `/v1/chat/completions` are peers
> over it; neither is expressed in the other's objects. This matters because
> every MLX Responses implementation in the wild (mlx-lm PR #1207, mlx-serve)
> lowers Responses onto Chat, which is the exact lossy path that breaks Codex
> custom tools.
>
> **Done:** legacy runtime deleted (`manager.py`, `responses.py`, all
> `upstream_*`/`responses_engine` config, the `:7334` proxy) with
> `tests/test_no_second_runtime.py` as a standing guard; first-class Chat in
> `chat_api/` incl. tool calling and explicit rejection of what it cannot
> represent; real per-generation phase timings and rolling aggregates fed by
> both surfaces, exposed at `/v1/synth/inference`, its SSE stream, `/metrics`,
> and `POST /v1/synth/model/unload`; the Desktop Inference panel as both a chat
> right rail and an Inventory tab; and the stuck-`Working` race fixed with an
> atomic `codex_turn_send` command.
>
> **Corrections to the plan below.** The idle-unload default moved from 30s to
> **900s**, matching the measured Poolside sidecar — 30s evicts a 20 GB model
> between turns. The `/v1/synth/status` route is gone with the process manager.
> Poolside's own sidecar is on **`:63300`**, not `:7334`; `:7334` was only ever
> our managed child. Chat cannot be served by the remote-passthrough backend,
> which forwards the original Responses body upstream — it returns
> `chat_requires_local_backend` rather than fabricating one.
>
> **Known divergence worth a decision:** Chat rejects non-zero
> `presence_penalty`/`frequency_penalty` because the MLX sampler exposes only
> temperature and top_p, while `/v1/responses` still accepts and silently
> ignores them. Chat's behavior is the one the contract asks for; Responses
> should probably follow, but changing it may affect the compliance suite and
> was left alone.
>
> **Still open:** the OpenResponses compliance suite has not been re-run (the
> pinned checkout is not present on this machine), and Phase F — the
> current-source Codex fixture matrix, the CUA/Craftax acceptance, and the
> canonical app replacement — has not been run. Counts as of this update:
> Laguna 125 (24 live-suite skips), Rust lib 113, renderer 47, typecheck clean.

## Outcome to ship

`services/laguna-daemon` must be the only local Laguna runtime:

```text
Synth Desktop
    |
    | OpenAI/OpenResponses HTTP, SSE, and WebSocket on 127.0.0.1:7333
    v
Laguna daemon
    |
    | typed Responses turns and canonical ModelEvents
    v
NativeMlxBackend
    |
    v
MLX weights loaded in this process
```

There must be no normal local path that starts, manages, or proxies through a
second `mlx_lm.server` on `127.0.0.1:7334`. Poolside's closed-source sidecar is
not a dependency or a supported black box. It was useful only as behavioral
reference. A stock `mlx_lm.server` is also not the target architecture.

The final product has one daemon process, one model runtime, one admission
queue, one prompt-cache owner, and one source of residency and inference
telemetry truth.

## Contract decisions that are not open for reinterpretation

- OpenResponses `2026-04-24` / `2.3.0` is the portable core.
- The OpenAI/Codex extension profile is required for Synth. It covers custom
  tools, namespace/deferred tool search, MCP, shell/apply-patch, Codex model
  metadata, lifecycle endpoints, and richer stream events.
- Responses items are the canonical representation. The native path must never
  lower a request into a Chat object or make an internal
  `/v1/chat/completions` request.
- Model tool-name lowering is allowed only inside prompt compilation. The
  immutable `ToolBinding` table must restore the exact original call kind.
- Unsupported model capabilities return stable explicit errors. Do not ignore
  modalities, formats, tools, or continuation fields.
- Hosted shell/apply-patch remain disabled in the inference daemon and are
  delegated to Codex/Desktop. Hosted MCP requires a separately secured,
  allowlisted executor; do not turn arbitrary model output into network access.
- Local inference uses the standard/default service tier.
- The canonical installed Desktop app is replaced only after the complete
  current-source standard gate and live acceptance gate are green.

## Current repository state

The worktree is intentionally dirty and has concurrent Desktop, visual, trace,
container, and Laguna work. Inspect `git status --short` and relevant diffs
before every edit. Do not reset, discard, or mechanically rewrite unrelated
changes.

Important current implementation files:

- `laguna_daemon/app.py`
- `laguna_daemon/config.py`
- `laguna_daemon/manager.py`
- `laguna_daemon/responses.py`
- `laguna_daemon/responses_api/`
- `tests/test_native_responses.py`
- `scripts/run_codex_e2e.py`
- `../../apps/synth_desktop/src-tauri/src/laguna.rs`
- `../../apps/synth_desktop/src-tauri/src/codex.rs`
- `../../apps/synth_desktop/src/renderer/src/App.tsx`
- `../../apps/synth_desktop/src/renderer/src/components/InventoryPage.tsx`
- `../../apps/synth_desktop/src/renderer/src/components/LocalModelResidency.tsx`

### Implemented native server baseline

The native stack currently includes:

- pinned schemas, licenses, hashes, and generated/validated overlay types;
- typed Responses validation and capability errors;
- semantic HTTP/SSE with stream/non-stream final-object equivalence;
- SQLite WAL persistence, retrieval/deletion, pagination, and
  `previous_response_id`;
- token counting, cancellation, background responses, compaction, and
  WebSocket continuation/recovery;
- function, custom, namespace/tool-search, MCP, shell, and apply-patch item
  lifecycles;
- strict JSON/JSON Schema output and grammar-backed custom tools;
- native MLX model loading, bounded admission, prompt caches, idle unload, and
  honest residency;
- redacted completed-response telemetry;
- Codex request fixtures and deterministic fake-backend coverage.

Recent source fixes that must remain:

1. `ResponsesCoordinator` closes the backend iterator with
   `contextlib.aclosing`, including sink and client-disconnect failures. This
   prevents a leaked generation slot.
2. Tool-bearing model output suppresses trailing assistant prose. Codex treats
   assistant prose in the same response as terminal and otherwise fails to
   continue the tool loop.
3. Custom-grammar activation decodes only a bounded generated suffix instead
   of re-decoding the entire 10k+ token prompt on every generated token.
4. After a complete tool envelope the sampler allows a 16-token grace window
   for an adjacent parallel call, then stops before unbounded trailing prose.
5. Laguna tokenizer loading enables `fix_mistral_regex=True`.

These fixes were committed to source after the currently observed long-lived
`:7333` daemon was started. A clean daemon restart is required before drawing
live performance conclusions from them.

### Prompt/tool advertisement finding

The normal starting prompt is roughly 10k tokens, not 50k. A measured first
Codex call with both selected skill bodies loaded was `12,198` input tokens.
Keep skills lazy-loaded and their catalog descriptions short; do not delete
useful instructions merely to optimize a manageable baseline.

The raw visuals MCP defines 14 tools and serialized to 3,779 characters. The
generated Codex config now advertises only the compact `visual_manage` facade,
about 416 characters, while retaining legacy implementation tools behind that
facade. This saves about 3,363 advertised characters. The containers MCP has
five tools and approximately 1,639 total serialized characters. Preserve the
compact facade and description-budget tests.

## Exact verified evidence

The following evidence was green during this work. Re-run it after the deletion
and telemetry changes; it is a baseline, not permission to skip the final gate.

- Laguna native suite after the latest sampling changes: **42 passed, 0
  failed**, 0.292 seconds, with one Starlette deprecation warning.
- Schema/license/type validation: **4 schema/license pins and generated types
  validated**.
- Pinned OpenResponses compliance repository:
  `openresponses/openresponses@cd31bc2060a27ee87a05ec97f49c84027eb6c3ba`.
- OpenResponses compliance against a deterministic native mock on `:7340`:
  **17 passed, 0 failed, 0 skipped**.
- Official SDK smoke tests:
  - Python `openai==2.53.0`: completed, one output item.
  - Node `openai@7.4.0`: completed, one output item.
- Full Desktop verification at the observed baseline:
  - TypeScript typecheck passed.
  - Rust library tests passed (91 at that run; later targeted additions bring
    the current discovered total higher, so use the new run's exact count).
  - `synth-containers-mcp`: 1 passed.
  - `synth-visuals-mcp`: 3 passed.
  - protocol tests: 35 passed.
  - real-bundle trace test: 0 passed, 1 intentionally ignored.
  - Playwright: 57 passed.
- Later targeted loopback rollout security test: 1 passed, 92 filtered.

Commands:

```bash
./scripts/laguna/test.sh
services/laguna-daemon/.venv/bin/python \
  services/laguna-daemon/scripts/check_schemas.py

npm run desktop:verify
npm run typecheck --workspace @synth/synth-desktop
npm run frontend:build --workspace @synth/synth-desktop
```

The compliance command, from the pinned upstream checkout, is:

```bash
bun run test:compliance \
  --base-url http://127.0.0.1:7340/v1 \
  --api-key openresponses-test \
  --model poolside/Laguna-XS-2.1-NVFP4-mlx
```

## Evidence that is still missing

Do not claim the cutover complete until all of this is newly captured against
the final source:

- The full `scripts/run_codex_e2e.py` fixture matrix has not been re-run after
  the latest native MLX fixes. The live daemon's single generation slot was
  occupied, and the working Desktop task was deliberately not stopped.
- The final installed-app CUA acceptance has not been re-run against the final
  one-sidecar build.
- The current run has no final pair of Craftax rollout IDs and no final rollout
  or evaluation visual IDs to report.
- The canonical Synth Desktop app was not reinstalled/replaced by this run.
- An older container handoff says a canonical live gate passed on 2026-08-09.
  Treat that as historical evidence only. It predates the final source and does
  not satisfy this cutover's re-run requirement.

At the last observation, a native MLX process used roughly 22 GB while macOS
had heavy compressed-memory pressure. Starting a second 20 GB server is unsafe
and invalidates the architecture. Drain or finish the existing task, restart
the single daemon cleanly, and continue with one runtime.

## Phase A: delete the second local runtime

The legacy architecture still exists in:

- `LagunaProcessManager` in `laguna_daemon/manager.py`;
- `upstream_host`, `upstream_port`, and `upstream_url` in
  `laguna_daemon/config.py`;
- `SYNTH_LAGUNA_RESPONSES_ENGINE=legacy` branches in
  `laguna_daemon/app.py`;
- the direct `/v1/chat/completions` proxy and helpers in
  `laguna_daemon/app.py`;
- the Chat translation module `laguna_daemon/responses.py`;
- tests that still construct an upstream on port 17999.

Required deletion/cutover:

1. Make native Responses unconditional for local `mlx_lm` operation.
2. Remove `SYNTH_LAGUNA_RESPONSES_ENGINE=legacy` and all local managed-process
   startup, shutdown, idle watch, status, log, and `:7334` configuration.
3. Delete `LagunaProcessManager` if no supported external-provider path needs a
   narrowed replacement. A remote provider must use a native Responses
   passthrough adapter; it must not preserve local process-management concepts.
4. Delete the legacy Chat-to-Responses translator after its rollback window.
   If release policy still requires a rollback for one build, quarantine it
   behind a build-time feature that cannot spawn a local server and give it an
   explicit removal issue/date. Do not maintain two production runtimes.
5. Update Desktop's Laguna manager so it supervises only the daemon on `:7333`.
6. Add a negative test that scans the production local path and proves it
   cannot import/spawn `mlx_lm.server`, bind `:7334`, or make a local upstream
   HTTP request.

Acceptance:

```text
lsof :7333 -> exactly one Laguna daemon
lsof :7334 -> no listener
process tree -> no python -m mlx_lm.server child
first prompt -> NativeMlxBackend loads weights in :7333 process
idle expiry -> weights and prompt caches released, daemon remains alive
next prompt -> same daemon reloads and completes
```

## Phase B: retain Chat Completions only as a native adapter

Keep `/v1/chat/completions` only if an actual supported client still requires
it. Implement it as a boundary adapter over the in-memory native Responses
service:

```text
ChatCompletion request
  -> validate supported Chat subset
  -> construct typed ResponseRequest
  -> call ResponsesService directly (no HTTP loop)
  -> transform final Responses items or semantic events to Chat response/chunks
```

Rules:

- Chat types exist only at this endpoint boundary.
- Persistence, admission, token accounting, cancellation, model residency,
  prompt caching, and telemetry remain owned by `ResponsesService` and
  `NativeMlxBackend`.
- Reject Chat fields that cannot be represented faithfully.
- Never use the Chat adapter for Codex or native `/v1/responses` requests.
- Streaming Chat and non-streaming Chat must reconstruct the same final text,
  finish reason, tool calls, and usage for the supported subset.

Add tests proving Chat works while `LagunaProcessManager`, `subprocess.Popen`,
`mlx_lm.server`, and upstream HTTP are unavailable.

## Phase C: make residency and inference telemetry authoritative

`GET /health` already exposes native residency fields. Preserve them and source
them exclusively from `NativeMlxBackend`:

- `loadedModel`
- `memoryBytes`
- `idleSeconds`
- `lastUsedAt`
- `freeAt`
- `idleUnloadAfterSeconds`

Keep `SYNTH_LAGUNA_IDLE_UNLOAD_SECONDS`; the temporary development default is
30 seconds. Active generation and token-count work must hold an eviction guard.
Unload the model, prompt caches, grammar/tokenizer resources as appropriate,
and MLX cache without terminating the daemon.

The current `/v1/synth/responses/telemetry` is only a 256-entry deque of
completed response summaries (`latency_ms`, input/output tokens, item count,
status, and error). Expand observability before building the Desktop pane.

Recommended redacted contract:

```json
{
  "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
  "resident": true,
  "residentBytes": 21568899389,
  "queueDepth": 2,
  "queueCapacity": 8,
  "active": {
    "generationId": "sha256:short-redacted-id",
    "phase": "prefill",
    "queuedAt": 0,
    "startedAt": 0,
    "firstTokenAt": null,
    "lastTokenAt": null,
    "promptTokens": 12198,
    "cachedTokens": 0,
    "outputTokens": 0,
    "cacheHitRatio": 0.0,
    "prefillTokensPerSecond": null,
    "decodeTokensPerSecond": null,
    "elapsedMs": 0
  },
  "rolling": {
    "requestsCompleted": 0,
    "requestsFailed": 0,
    "requestsCancelled": 0,
    "inputTokens": 0,
    "outputTokens": 0,
    "cachedTokens": 0,
    "ttftP50Ms": null,
    "ttftP95Ms": null,
    "decodeTpsP50": null,
    "decodeTpsP95": null,
    "latencyP50Ms": null,
    "latencyP95Ms": null
  }
}
```

Use the backend's actual timestamps and token counters. Distinguish queue wait,
model load, prompt compilation, prefill, first-token latency, and decode. Do not
derive fake GPU utilization from process CPU, and do not call an expensive OS
sampler in the generation loop. If reliable Apple GPU counters are unavailable,
show `Unavailable` rather than an invented percentage.

Never include prompt text, reasoning text, custom-tool input, tool output,
credentials, file contents, or complete stable response IDs in telemetry.
Bound all histories and label whether rolling aggregates reset on daemon restart.

## Phase D: add the Desktop inference monitor

Add an **Inference** sibling to Containers, Traces, Visuals, and Usage. The goal
is a readable btop-like local-model surface, not a developer JSON dump.

Suggested layout:

```text
Inference · Laguna XS 2.1                     RESIDENT · 20.1 GB

GENERATING   decode                 12.4 tok/s       18.2 s
[queue 2/8] [prompt 12,198] [cached 8,420 · 69%] [output 226]

TTFT        1.84 s p50   3.10 s p95
Decode      12.4 p50     13.1 p95 tok/s
Requests    31 ok        1 failed   2 cancelled

throughput sparkline          queue-depth sparkline
recent requests: phase, model, tokens, cache, TTFT, TPS, status
```

Implementation shape:

- Add an `inference` inventory/output tab and a focused
  `InferencePane.tsx` component rather than growing `InventoryPage.tsx` into a
  monolith.
- Add typed Rust/TypeScript bridge models. Do not pass arbitrary JSON through
  the renderer boundary.
- Prefer a daemon SSE status stream for the active btop view. A 500-1000 ms poll
  is acceptable initially, but only while the pane is visible; stop all polling
  when it closes or the window is hidden.
- Keep `/health` low-frequency and authoritative for residency. Use the
  inference endpoint/stream for high-frequency activity.
- Render queue/load/prefill/decode/cancel/failed distinctly.
- Make zero/unavailable states explicit and accessible. Honor reduced motion.
- Include a compact link from the existing Residency card to the full pane.

Tests:

- deterministic fake-backend timeline for load -> queue -> prefill -> decode ->
  complete;
- cache miss then cache hit with exact counters and ratio;
- cancellation and failure counters;
- idle unload clears resident state but preserves bounded completed aggregates;
- renderer test for loading, live generation, idle, unloaded, error, and
  unavailable metrics;
- polling/stream teardown when pane closes;
- no secret/prompt fields in serialized telemetry;
- Playwright screenshot at stable fixture timestamps and reduced motion.

## Phase E: fix `Codex session not started` leaving the UI Working

Observed screenshot:

```text
Working...   Stop
Codex session not started: 922c25f7-fef6-49e1-83d2-631554a04116
```

The immediate renderer bug is in `apps/synth_desktop/src/renderer/src/App.tsx`.
`sendToSession` appends the optimistic user event and sets the session to
`running` before `nativeCodex.startTurn` succeeds. Its `catch` only shows a
toast, so a failed turn start leaves `activeChatRunning` true indefinitely.

The Rust error originates in `CodexManager::session` in
`apps/synth_desktop/src-tauri/src/codex.rs`. Durable records survive, but the
in-memory `sessions` map contains only currently attached app-server processes.
An attachment can exit after `start()` observes it and before `start_turn()`
looks it up. Restart reconciliation work already marks detached records and
SQLite runs interrupted; preserve those changes, but also close this live race.

Required behavior:

1. Make attach/resume plus turn start atomic from the renderer's perspective.
   Prefer one Tauri command that ensures the app-server attachment and starts
   the turn under a per-session lock. If the child exits, return a typed
   `codex_session_detached` error and reconcile durable state before returning.
2. Do not set `running` until `turn/start` returns a real `turnId` or a
   `run.started` event arrives.
3. If an optimistic user message is retained for responsiveness, mark it
   unsent/failed on rejection and offer Retry. Do not silently remove user text.
4. On any start-turn failure, reconcile the session to `interrupted` or `ready`,
   hide Stop, re-enable the composer, and prevent stale `run.started` replay from
   resurrecting Working.
5. Replace the raw UUID toast with a useful message such as “The local agent
   process disconnected before the turn started. Retry to reconnect.” Keep the
   typed code and session ID in debug logs.
6. Stop remains idempotent for already-detached sessions.

Regression tests:

- app-server exits between ensure/start and turn/start;
- restored record says running but has no attachment;
- JSON record is already interrupted while SQLite run is still active;
- failed start never leaves `session.status === running`;
- composer re-enables, Stop disappears, text remains retryable;
- a subsequent Retry reattaches/resumes the same Codex thread and succeeds;
- Playwright fixture matching the screenshot state.

## Phase F: current-source Codex and Craftax live gate

After all deterministic suites pass, restart the daemon so it contains the
latest native source. Use a numbered app such as Test 1, Test 2, or Test 3; do
not use or replace the default/canonical app during diagnosis.

Run `scripts/run_codex_e2e.py` and verify, with real captured Codex traffic:

1. ordinary function call and output continuation;
2. custom tool call preserving raw input and identity;
3. namespace/deferred tool search;
4. shell and apply-patch in a disposable workspace;
5. MCP echo/sum dispatch with exact call counts;
6. cancellation/disconnect recovery;
7. restored-session continuation.

Then use Computer Use in the installed numbered Synth Desktop app. Do not click
Stop while the agent is working. Craftax must be available at
`http://127.0.0.1:8098`.

Final user-level task:

- the coding agent discovers or registers Craftax through `synth_containers`;
- there is no shell fallback for container discovery or rollout dispatch;
- it runs exactly two live rollouts;
- it creates a rollout visual and an evaluation/comparison visual;
- it opens the result in the visual pane;
- the new Inference pane shows the same request activity and honest metrics;
- the app reaches a terminal ready state without the detached-session bug.

Capture and report:

- numbered app identity and build path;
- daemon PID and proof that `:7334` is unused;
- exact test commands/counts;
- Codex session/thread/turn IDs;
- container ID;
- exactly two engine-provided rollout IDs;
- rollout visual ID;
- evaluation/comparison visual ID;
- screenshots of the final visual and Inference pane;
- any warning, capability rejection, or performance limitation.

Only after this gate is green should the canonical Synth Desktop app be
reinstalled/replaced.

## Definition of done

- [ ] One Laguna daemon listens on `127.0.0.1:7333`.
- [ ] No local code path or child process uses `mlx_lm.server` or `:7334`.
- [ ] `/v1/responses` is native and all portable/extension suites pass.
- [ ] `/v1/chat/completions`, if retained, adapts directly over native
      Responses and has no second runtime.
- [ ] Native residency load -> guarded idle unload -> reload is verified.
- [ ] Inference pane shows live phases, queue, token throughput, TTFT, cache,
      residency, and bounded redacted history.
- [ ] Detached Codex start failure cannot leave Working/Stop stuck.
- [ ] Current-source Codex fixture matrix passes against real MLX.
- [ ] Current-source CUA task completes exactly two Craftax rollouts and opens
      both required visuals.
- [ ] Exact IDs/evidence are recorded.
- [ ] README and operational rollback notes describe only the final supported
      architecture.
- [ ] Canonical app replacement occurs only after every item above is green.

