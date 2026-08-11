# Handoff: make Laguna XS 2.1 first-class in Workshop

Date: 2026-08-09 (later session)
Supersedes the plan sections it contradicts in
[`HANDOFF_NATIVE_MLX_CONSOLIDATION.md`](./HANDOFF_NATIVE_MLX_CONSOLIDATION.md);
read that one for the original contract decisions, this one for current state.

Audience: the engineer maintaining and extending local-model support in Synth Desktop.

---

## TL;DR

The daemon is now a single self-contained MLX runtime with two peer wire
surfaces (Responses and Chat) over one neutral core. Deterministic, compliance,
native Codex, mixed-protocol live, and sustained soak gates are green. Harbor
is wired directly to the local provider and exercised against real Craftax;
the remaining Craftax gap is Laguna XS agent strategy/score quality, not
transport, tool dispatch, cache reuse, cancellation, or runtime stability.

| Suite | Count | Status |
|---|---|---|
| Laguna deterministic (`./scripts/laguna/test.sh`) | 157 (25 live skips) | green |
| OpenResponses compliance @ `cd31bc2` | 17 / 17 | green |
| SDK smoke (Python 2.53.0, Node 7.4.0) | 2 / 2 | green |
| Desktop Rust lib | 134 | green |
| Desktop renderer (`node --test`) | 47 | green |
| `npm run typecheck` | — | clean |
| Desktop Playwright | 85 / 85 | green |
| Live MLX (`./scripts/laguna/live_test.sh`) | 25 / 25 | green |
| Mixed Chat/Responses soak (`./scripts/laguna/soak_test.sh`) | 12 iterations + recovery | green |
| Codex CLI over native Responses | 5 / 5 | green |
| Evals local-provider tests | 15 / 15 | green |

The canonical signed app is installed at `/Applications/Synth Desktop.app`.
Its bundled backend and capability files were byte-compared with current
source. An authenticated production probe returned 400 for 32769 output tokens
and a real MLX request completed HTTP 200 with exactly `OK`; one bundled daemon
owns `127.0.0.1:7333`.

---

## Resolved after this handoff was written

The pending reasoning-split patch is applied and its temporary patch file is
deleted. Thinking-enabled turns now trust Laguna's template-opened reasoning
span, so a checkpoint response with no opening `<think>` marker and only a
closing `</think>` cannot leak chain-of-thought into assistant content.

The final live run also exposed a fourth defect: concurrent requests could use
the Hugging Face fast tokenizer during another generation and fail with
`model_generation_failed: Already borrowed`. Prompt compilation, token
counting, custom-grammar setup, and generation now share the single owned
`laguna-mlx` executor. The three-request live gate passes, and
`test_tokenizer_work_uses_the_owned_mlx_thread` is the deterministic guard.

---

## Architecture: what is settled and why

### The sidecar boundary is a first-class acceptance seam

Laguna is supervised by Desktop, but it is independently addressable over a
small authenticated loopback API. Use that boundary aggressively: it lets the
MLX runtime be tested as a black-box product without renderer, Codex UI, or
Tauri timing contaminating the result.

The sidecar battery should emulate real workloads, not only route-level smoke:

- interleave streaming and non-streaming Chat and Responses requests;
- replay multi-turn Codex/tool traces with large, cacheable prefixes;
- exercise concurrent admission, queue saturation, disconnects, cancellation,
  slot recovery, unload/reload, and supervisor restart;
- assert semantic equivalence of reconstructed streams and non-stream results,
  correct tool-call arguments, finish reasons, usage, and error contracts;
- sample TTFT, decode throughput, cache-hit ratio, latency distributions,
  resident memory, post-warmup growth, and recovery time;
- compare performance to checked baselines with tolerant regression bands, not
  brittle single-run constants, and retain machine-readable reports for trend
  tracking.

Keep three layers distinct: deterministic unit/contract tests for exact edge
cases, bounded real-weight live tests for MLX correctness, and longer mixed
traffic soak/benchmark runs for resilience and trends. A failure at this seam
is actionable as a daemon/model/runtime failure; a passing seam sharply narrows
Desktop failures to supervision or presentation.

### Two peer surfaces over a neutral core

```
POST /v1/responses  ──┐                                ┌── ResponseEventAssembler → items + SSE
   items_to_messages  ├→ CompiledTurn → TurnRunner ────┤    (+ SQLite store, previous_response_id)
POST /v1/chat/…     ──┘   (neutral core)     ModelEvent └── ChatEventAssembler → chat.completion(.chunk)
   messages pass through                                    (stateless — no store)
```

Chat is **not** an adapter over Responses. The original plan proposed building
it by constructing a typed `ResponseRequest` and calling `ResponsesService`;
that was not built, because the neutral seam already existed. `compile_turn`
did exactly one Responses-specific thing (`items_to_messages`), and everything
after operated on a plain chat-shaped message list. So:

- `responses_api/compiler.py` — `compile_messages()` is the neutral core;
  `compile_turn()` is the Responses front-end onto it.
- `responses_api/runner.py` — `TurnRunner` owns neutral execution: the
  in-flight registry, cancel propagation, the `aclosing` slot release, and
  telemetry. Both surfaces run on the **same instance**, which is what makes
  residency, the single GPU slot, cancellation, and token accounting shared
  rather than duplicated.
- `chat_api/` — Chat types exist here and nowhere else.

**Why this matters and is worth defending:** every MLX Responses
implementation in the wild lowers Responses onto Chat —
[mlx-lm PR #1207](https://github.com/ml-explore/mlx-lm/pull/1207) states it
translates into the Chat pipeline "requiring no changes to the generation
engine" and filters unsupported tool types;
[mlx-serve](https://github.com/ddalcu/mlx-serve) says the same. That is the
exact lossy path `RESPONSES_SERVER_PLAN.md:38-41` documents as breaking Codex:
a `custom` tool lowered to a Chat `function` returns as `function_call` and
Codex refuses to dispatch the MCP bridge. Guardrail tests in
`tests/test_neutral_core.py` fail if Responses concepts leak into the core.

### One runtime

`manager.py` (the `mlx_lm.server` child), `responses.py` (the legacy
translator), all `upstream_*` / `responses_engine` config, and the `:7334`
proxy are **deleted**. `tests/test_no_second_runtime.py` is a standing guard:
the production package cannot import `subprocess`/`multiprocessing` at all, and
cannot reference `mlx_lm.server` or `:7334`.

> **Process-list hygiene.** `:7334` was *ours*. Poolside's own
> `poolside-mlx-sidecar` runs on **`:63300`** from `/Applications/Poolside.app`
> — not ours, not a dependency, never to be reused or killed. Its entire
> surface is three routes (`GET /health`, `GET /v1/models`,
> `POST /v1/chat/completions`); it has **no** Responses API. We are a strict
> superset, so "Poolside parity" is not a meaningful target any more.

### Chat's deliberate limits

Chat rejects rather than degrades, because degrading silently returns something
the caller did not ask for:

- Responses-only tool kinds (`custom`, `namespace`, `mcp`, `shell`,
  `apply_patch`) → `unsupported_chat_field`. Chat cannot carry a custom tool's
  raw input or a namespaced call's identity back.
- `n>1`, `logprobs`, `logit_bias`, `seed`, audio/image modalities,
  `prediction`, `web_search_options`, `stop`, non-zero presence/frequency
  penalties (the MLX sampler exposes only temperature and top_p), `store`.
- On an `external` (remote passthrough) backend, Chat returns
  `chat_requires_local_backend`. That backend forwards the original Responses
  body upstream, so serving Chat would mean fabricating one.

---

## Four bugs the live suite found

These are the reason the live suite exists. All four were invisible to the
deterministic suite, and two were actively harmful in normal use.

### 1. Generation-slot stranding (fixed)

`NativeMlxBackend.stream()`'s `finally` awaited the worker future before
releasing the admission slot. An `await` inside an **already-cancelled**
coroutine — the ordinary client-disconnect path — returns without joining the
thread. The slot reopened while an orphaned generation still owned the single
`ThreadPoolExecutor` worker, so the next request took the slot, queued its
worker behind the orphan, and sat in `prefill` forever.

Observed: a **51-token** prompt stuck in `prefill` for 7+ minutes at 0.1% CPU
with three requests queued behind it. The daemon stayed wedged after every
client was killed and needed a restart.

Fix: `_retire_generation` is attached to the worker future via
`add_done_callback`, so the slot's lifetime equals the thread's lifetime,
cancelled or not. Regression coverage in
`tests/test_api_compat.py::GenerationSlotOwnershipTests`.

**Result: the concurrency test went from hanging indefinitely to 2.3 s.**

### 2. Streaming was not streaming (fixed)

Measured: a 12.7-second generation arrived as **3 SSE frames** — the role
frame, one 1280-character content frame at t=12.672s, and the finish frame.

Cause, in the consumer loop:

```python
if not reasoning_complete:
    if "</think>" not in pending:
        continue          # buffers every chunk, forever, if no </think> arrives
```

`reasoning_complete` starts `False` for any non-structured turn, so nothing was
emitted until a closing marker appeared — and this checkpoint often emits none.
Streaming silently degraded to non-streaming for both surfaces.

Fix: `_IncrementalReasoningSplitter` holds back only what is genuinely
ambiguous — a boundary-sized window that might contain a partial `</think>`.
Deterministic coverage in `tests/test_incremental_streaming.py`; the live guard
is `test_streaming_is_actually_incremental`, which asserts many frames spread
over real time rather than only checking reassembled text.

**Result: 3 frames → 258 frames, arriving ~46 ms apart over ~9 s**, matching
the daemon's measured 48 tok/s. The client-observed throughput figure went from
a nonsense 238,426 tok/s to a sane 30.9.

### 3. `residentBytes` measured the filesystem (fixed)

It summed the model files' **on-disk** size and reported it as memory — so the
Desktop panel would show "20.1 GB resident" regardless of what was actually
allocated. Process RSS at the time was 0.06 GB.

Fix: `NativeMlxBackend.memory_bytes()` uses `mx.get_active_memory()`, the real
allocator figure, and returns `None` when it cannot be measured. It now reports
20.08 GB — the number happens to agree, but it is now actually measured. RSS
under-reports Metal buffers on Apple silicon, which is why RSS is not used.

### 4. Concurrent tokenizer borrowing (fixed)

Three simultaneous Chat requests could overlap prompt/tokenizer work with an
active MLX generation. Hugging Face's fast tokenizer is not re-entrant, so one
request intermittently returned HTTP 500 with `Already borrowed`.

Fix: all tokenizer-touching work now runs on the backend's one owned
`laguna-mlx` executor. Live serialization passes; deterministic coverage checks
the executor thread identity.

---

## Telemetry contract

The Desktop panel is built against this; changing it silently blanks the panel,
so `tests/test_inference_telemetry.py` asserts the field set.

- `GET /v1/synth/inference` — `{model, resident, residentBytes, queueDepth,
  queueCapacity, active{…}|null, rolling{…}, gpuUtilization}`.
  `active.phase` ∈ `queued | loading | compiling | prefill | decode | complete`.
- `GET /v1/synth/inference/stream` — same payload as SSE.
- `GET /metrics` — Prometheus exposition of the same numbers.
- `POST /v1/synth/model/unload` — explicit release; `409` while a generation is
  in flight.

**Rules that are load-bearing, not stylistic:** every metric is measured or
`null`; `null` means genuinely unavailable and must render as "Unavailable",
never as `0` or an interpolation. `gpuUtilization` is `null` — Apple GPU
counters are not reliably available and deriving one from process CPU would be
a fabrication. Telemetry never carries prompt text, reasoning text, tool
input/output, credentials, file contents, or a complete response id
(`generationId` is a truncated SHA-256).

`SYNTH_LAGUNA_IDLE_UNLOAD_SECONDS` now defaults to **900** (was 30, matching
the measured Poolside sidecar). 30 s evicted a 20 GB model between turns.

Active telemetry is genuinely live: `outputTokens` and decode throughput are
updated for every MLX chunk rather than only when a response finishes.

## Output budget and tool-dialect resilience

Both APIs now default to **8192 output tokens** and enforce a hard maximum of
**32768**. The former 1024-token implicit limit truncated large Codex tool
calls and induced deterministic retry loops; values above the hard cap now
return a validation error on every request shape, including extension tools.

The Responses tool normalizer remains fail-closed, but accepts the exact
bounded aliases observed in real Harbor traces:

- `read(path)` and `read_file(path)` lower to a quoted, macOS-safe `sed` read.
- `grep(pattern, path, output_mode)` lowers to a quoted `rg` invocation.
- `write(path, input)` and `write(path, contents)` lower to a parent-creating,
  base64-encoded command, with a 4096-character path and 1 MiB content bound.

Unknown tools and argument spellings are still rejected. Unit tests cover each
accepted spelling, and real Craftax traces proved both write dialects.

## Full lifecycle acceptance

The sidecar lifecycle is now exercised both below the UI and through the
installed app. A CUA pass on 2026-08-09 used the actual residency controls:
`Free now` released the 20.1 GB allocation, the next message cold-loaded the
checkpoint, completed in 12 seconds with `COLD_OK`, and reset the 15-minute
idle deadline. The inference monitor and sidebar both reflected the transition.
Cold local turns now say **Warming up…** in the transcript until Laguna reports
resident weights; **Working…** is reserved for inference after warmup.

When no complete local checkpoint is discovered, Settings now offers
**Download from Hugging Face**. It uses the managed Laguna Python environment,
downloads the pinned public revision, validates the completed indexed shards,
and selects that copy. A portable `df` preflight requires 24 GiB of free disk
space before the transfer begins. The UI bridge and disk parser have regression tests; do not
replace it with the old fixture-only progress bar.

Before importing MLX or allocating weights, the native backend now checks
installed unified-memory capacity. It requires at least 32 GiB, or indexed
weight size plus 8 GiB of headroom when that is larger. Unsupported machines
receive a stable `insufficient_system_memory` 503 and the model remains
unloaded. The check deliberately uses physical capacity rather than volatile
"free pages," which excludes reclaimable cache and creates false rejections on
macOS. A deterministic 16 GiB regression test proves the gate happens before
model loading.

The production installer also stops the exact PID in the Synth-managed Laguna
pid file after validating that its command is `-m laguna_daemon`. Without this,
replacing Desktop could orphan the old daemon and the new app would adopt its
healthy port while it continued executing stale in-memory Python. Foreign
processes and Poolside's sidecar remain untouched.

---

## Measured baselines (real weights, M-series, 2026-08-09)

Comparable runs write to `SYNTH_LAGUNA_LIVE_REPORT`.

| Metric | Value |
|---|---|
| Decode p50, live suite (daemon-measured) | 37.3 tok/s |
| Decode p50 / p95, soak (daemon-measured) | 38.4 / 55.1 tok/s |
| Resident | 20.08 GB |
| Prompt-cache reuse | 5545 / 5552 tokens |
| Prompt-cache speedup | 6.82× (2.986 s → 0.438 s) |
| Cold reload after unload | 8.011 s |
| 200-token client-observed decode | 51.2 tok/s |
| 3 concurrent requests | 5.771 s (serialized on one GPU slot) |
| Soak max / median latency | 2.224 / 1.285 s |
| Soak post-warmup resident growth | 54,394,880 bytes (~51.9 MiB) |
| Post-disconnect follow-up | 0.491 s |

Trust the **daemon-side** decode figure. Client-side stream timing is only as
good as the client's flushing and is recorded separately as `client_*`.

---

## What remains, in order

### 1. Craftax score quality, not backend acceptance

The separate `evals` repo now has a first-class `laguna_local` Codex provider,
`SYNTH_LAGUNA_API_KEY`, exact local model metadata, local-environment matrix
support, and a checked low-effort Craftax matrix config. The GameBench Harbor
adapter also exports a truthful local-provider contract. Its staged Code Policy
instruction asks the agent to leave inspection after 12 reads and begin a
candidate/verifier loop.

Real Harbor/Craftax attempts exercised clean sequential turns above 70k prompt
tokens, ~99% prompt-cache reuse, custom tool dispatch, both observed write
dialects, strict rejection and successful retry of an unknown `timeout`
argument, and creation of a 171-line candidate. The daemon had no failures.

The bounded acceptance attempt was stopped because Laguna XS continued broad
inspection despite the explicit 12-read pacing instruction. That is an honest
model instruction-following/optimization limitation: do **not** call the
Craftax score green. Backend acceptance is green; improving the task score now
means prompt/policy/model work, not weakening the API or adding permissive tool
guessing.

### 2. CUA / Craftax acceptance (handoff Phase F)

A numbered instance is already built:

```
~/.synth-desktop/instances/test-1/build/target/debug/bundle/macos/Synth Desktop · test-1.app
```

Two things to know before driving it:

- It **shares the `:7333` daemon** by design (`HANDOFF_ISOLATED_DEV_INSTANCES.md`
  lists Laguna under "intentionally shared", to avoid duplicating 20 GB).
- Instance isolation is **env-var only** — there is no `LSEnvironment` in the
  Info.plist. Launching it by double-click or bare `open -a` runs it against the
  **canonical** data and state roots and silently shares the canonical
  database. It must be launched with `SYNTH_DESKTOP_INSTANCE`,
  `SYNTH_DESKTOP_DATA_ROOT`, `SYNTH_DESKTOP_CONFIG`, `SYNTH_CODEX_HOME`,
  `SYNTH_DESKTOP_WORKSPACE`, `SYNTH_DESKTOP_APP_NAME`,
  `SYNTH_DESKTOP_INSTANCE_MANIFEST`.
- `instance.json`'s `executable` points at the raw binary, not the bundle
  executable — relevant for CUA targeting.

The native `scripts/run_codex_e2e.py` gate is already green (five current
fixtures) against real MLX. The remaining UI-level Craftax task should
discover/register through `synth_containers` with no shell
fallback, exactly two live rollouts, a rollout visual and an
evaluation/comparison visual, Craftax at `http://127.0.0.1:8098`. Capture app
identity, daemon PID, `:7334` proof, test counts, session/thread/turn IDs,
container ID, both rollout IDs, both visual IDs, and screenshots. **Redact the
Laguna key — it appears in argv.**

### 3. The isolated lifecycle acceptance still open

From `apps/synth_desktop/HANDOFF_SESSION_LIFECYCLE_E2E.md`: start a long turn,
resolve the exact app-server child by parent chain, force-kill it, require
`/health` to report `inflight_generations == 0`, `queued_generations == 0`,
`generation_slot_available == true` within a short bound, then require a
follow-up to complete on the unchanged `codex_thread_id`. The slot-stranding
fix makes this materially more likely to pass than before.

---

## Making the experience *positively* first-class

Everything above gets it correct. The highest-value first-class UI work is now
implemented: the transcript renders incremental reasoning in a Thinking block,
loading/inference state is visible, and the Desktop acceptance suite covers the
live Thinking experience. Remaining polish, roughly in value order:

1. **Make the Inference panel more discoverable.** It is mounted as a chat right rail
   (titlebar toggle) and an Inventory tab. The residency chip in `Sidebar.tsx`
   is the more discoverable entry point but was owned by another workstream
   during this session; wiring it to open the rail is a small change.
2. **Token budgets in the UI.** Laguna's reasoning means a 64-token cap yields
   an empty answer with `finish_reason: length`. Anything in Workshop that sets
   `max_tokens` needs a reasoning-aware floor — this cost three false test
   failures before it was understood.
3. **Sampling defaults.** The checkpoint ships **no** `generation_config.json`,
   so there is no published recommendation, and the daemon defaults to
   `temperature=1.0, top_p=1.0`. Greedy (`temperature=0`) sends this model into
   repetition loops that run to the cap. The live suite uses **0.7 / 0.95** as
   a deliberate, documented choice. Consider making that the daemon default and
   surfacing it in Backend Settings.
4. **Queue feedback.** One GPU slot, capacity 9, then `model_queue_saturated`
   (429). Three concurrent requests take 14.3 s. The UI should show queue
   position rather than looking hung.
5. **`/metrics` is free observability.** Already exposed; nothing scrapes it.

---

## Verification

```bash
# Daemon
./scripts/laguna/test.sh                       # 156, 25 skipped
services/laguna-daemon/.venv/bin/python services/laguna-daemon/scripts/check_schemas.py
./scripts/laguna/live_test.sh                  # 25/25; needs a running daemon
./scripts/laguna/soak_test.sh                  # mixed APIs, cancellation, recovery, memory
SYNTH_LAGUNA_API_KEY=... services/laguna-daemon/scripts/run_codex_e2e.py \
  --base-url http://127.0.0.1:7333/v1         # 5/5

# Compliance (clone lives at /tmp/openresponses, pinned cd31bc2060a27ee87a05ec97f49c84027eb6c3ba)
bun run test:compliance --base-url http://127.0.0.1:7340/v1 \
  --api-key openresponses-test --model poolside/Laguna-XS-2.1-NVFP4-mlx

# Desktop
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib   # 134
npm --prefix apps/synth_desktop run typecheck
node --test apps/synth_desktop/tests/*.test.mjs                            # 47
npm run test:playwright --workspace @synth/synth-desktop                   # 85
```

Restarting the daemon on current source (the editable install keeps old code in
a live process, so a restart is required after any daemon change):

```bash
KEY=$(cat ~/.synth-desktop/laguna/api_key)
kill $(lsof -nP -iTCP:7333 -sTCP:LISTEN -t); sleep 3
cd services/laguna-daemon && PYTHONPATH=. nohup ~/.synth-desktop/laguna/.venv/bin/python \
  -m laguna_daemon --host 127.0.0.1 --port 7333 \
  --models-dir "$HOME/.config/poolside/models" \
  --default-model poolside/Laguna-XS-2.1-NVFP4-mlx \
  --api-key "$KEY" --backend auto > /tmp/laguna-7333.log 2>&1 &
```

---

## Traps

- **The worktree is intentionally dirty** with several concurrent workstreams
  (optimizers, visuals, containers, renderer polish). Never `git reset`,
  `checkout --`, `stash`, `clean`, or bulk `add`. `work/` is large scratch and
  must not be committed. Nothing in this session was committed.
- **Other workstreams break the build transiently.** `optimizers/service.rs`
  and `visuals/runtime/bind.ts` both failed to compile mid-session and fixed
  themselves. Retry before assuming a failure is yours.
- **`config.py` silently overrides an explicit `--models-dir`** when the given
  directory contains no `*.safetensors` and Poolside's weights exist — the
  "prefer existing Poolside weights" branch. Harmless under `backend=mock`,
  surprising under `mlx_lm` if you are deliberately isolating weights.
- **Responses still accepts and ignores** `presence_penalty` /
  `frequency_penalty`, while Chat rejects them. Chat's behavior is what the
  contract asks for; aligning Responses may affect the compliance suite, so it
  was left alone. Open decision.
- **`residentBytes` vs RSS.** `ps` shows ~0.06 GB for a fully loaded model
  because Metal buffers do not land in RSS. Use `mx.get_active_memory()`; do
  not "fix" this back to RSS or to on-disk size.
- Manage `/Applications/Synth Desktop.app` only through
  `./scripts/desktop.sh`; do not launch the generated build-tree bundle. Do not
  touch the `beta` instance or Poolside's `:63300` sidecar during acceptance
  runs.
