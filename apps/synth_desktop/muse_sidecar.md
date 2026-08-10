# Muse Glimmer — first-class Laguna sidecar integration

Status: **indefinitely paused for local use** (2026-08-10) — see
`MUSE_LOCAL_PAUSED.md` at the repo root for the measured rationale and the
resume checklist. Local weights and the managed runtime were removed from this
machine. The integration below remains accurate as documentation of the built
surface.

Prior status: shipped and installed (local `dev`, uncommitted; `/Applications/Synth
Desktop.app` rebuilt 2026-08-10). Muse is served by a Laguna-owned llama.cpp
backend behind the same `:7333` contract as Laguna XS. Responses and Chat
Completions both work, streaming and not, with tools, reasoning split, real
usage, and cancellation that reaches the engine. Verified cold: launching the
installed app starts the engine, brings up the daemon on `llama_cpp`, and
answers a live turn with no manual step.

Canonical model id (now the only one in the codebase):

```text
meta-models/Muse-Glimmer-30B-GGUF
```

Pinned llama.cpp (Muse + mmproj + DFlash): `dd1ea524333b1e697489067d7a4c39c60d32beee`  
Weights: `~/.synth-desktop/models/meta-models/Muse-Glimmer-30B-GGUF/`  
  - `muse-glimmer-30B-kquant-17gb.gguf` (~17 GB)
  - `mmproj-kquant.gguf`
  - `dflash-kquant.gguf`  
Managed runtime: `~/.synth-desktop/muse/runtime/llama-<commit>/llama-server`  
Engine args: owned by `spawn_muse_engine` in `laguna.rs`; `scripts/muse/serve.sh`
is the by-hand equivalent and must stay in agreement.  
Engine log: `~/.synth-desktop/laguna/muse-llama.log` · daemon log:
`~/.synth-desktop/laguna/desktop-sidecar.log`

## What shipped

| Layer | State |
| --- | --- |
| `LlamaCppChatBackend` implementing `ModelBackend` | **New** — `backends/llama_cpp.py` |
| `POST /v1/responses` (stream + non-stream, WS, lifecycle) | Works on Muse |
| `POST /v1/chat/completions` (stream + non-stream) | Works on Muse — peer, not a 501 |
| Tool calls (function / shell / apply_patch / custom / namespace) | Works; kinds restored from bindings |
| Reasoning split | `reasoning_content`, plus an inline `<think>` fallback |
| Usage / finish_reason | From the engine; never fabricated |
| Cancel + disconnect | Stops llama.cpp in ~0.6 s (measured) |
| Engine spawn | Direct from Rust, structured failure phases |
| Health honesty | `responsesApi`/`chatCompletionsApi` fail closed; `engine` block |
| Identity | One id everywhere; legacy spelling normalized on input |
| Desktop | Muse model id, provider label, compact limit, restore-on-reload |

## Architecture

Codex and every Chat client still talk only to Laguna on `:7333`:

```text
Desktop / Codex
    │  Responses (or Chat)
    ▼
Laguna daemon :7333
    │  one turn core, one admission slot, one cancel registry, one usage path
    ├─ NativeMlxBackend      → Laguna XS weights (in-process)
    └─ LlamaCppChatBackend   → Muse engine over loopback Chat Completions;
                               Responses synthesized by Laguna, never by Codex
```

The engine is a **token source**, not an upstream. It never sees a Responses
object, never decides an item's type, and never ends a turn. That is why it is a
backend and not `RemoteResponsesBackend`, which forwards a Responses body (502s
against llama.cpp) and 501s Chat.

### Why the old path was broken

`backend=external` bound Muse to `RemoteResponsesBackend`:

- Codex Responses → Laguna → `:7334/v1/responses` → 404/502
- `POST /v1/chat/completions` → `chat_requires_local_backend` → 501

Config now migrates that spelling: a Muse selection with only
`SYNTH_LAGUNA_EXTERNAL_URL` set is read as an engine address, not an upstream,
and `_make_backend` refuses to bind Muse to the passthrough at all.

### The one-runtime invariant still holds

`test_no_second_runtime.py` is unchanged in substance. The daemon does not
start, restart, discover, or supervise the engine, and does not know its port —
Desktop passes `SYNTH_LAGUNA_ENGINE_URL` in. A test asserts no `127.0.0.1`
literal exists in the backend.

## Contract details worth knowing

- **Both surfaces are advertised together.** They are peers over one backend, so
  any state that breaks one breaks both. `responsesApi` is false only when the
  engine is unreachable or erroring; `loading` keeps it true and reports
  `status: "loading"` with a human-readable `detail`.
- **Auth.** Desktop gives the engine the same bearer token that guards the
  daemon (`--api-key`), so no other local process can reach the weights through
  the engine's port.
- **Token counts** come from the engine's own tokenizer (`/apply-template` then
  `/tokenize`). A build without the template endpoint falls back to tokenizing
  message text, which is short by the template scaffolding and documented as
  such — never padded with a guess.
- **`memoryBytes` is null for Muse.** The weights are resident in another
  process; the GGUF's size on disk is a fact about the filesystem. `/health` now
  reports measured allocator bytes for MLX too, instead of summing files.
- **Residency policy (decided): the engine stays resident while Muse is the
  selected model.** Releasing on idle needs a lazy restart on the next turn, and
  the daemon cannot start a process; releasing without that path would trade a
  warm 17 GB for turns that fail until the user reselects. Memory is released
  when Muse is deselected, on model delete, and at app exit — all supervisor
  actions. `POST /v1/synth/model/unload` returns a typed
  `engine_release_not_supported` naming the owner rather than a false
  `generation_in_flight`.
- **Vision is not advertised.** The projector is loaded, but no wire surface
  lowers an image part to the engine, so the capability bit stays false.
- **Cancellation closes the connection, and the connection is per generation.**
  See below — this was the subtlest bug in the build.

### The cancellation trap (do not regress this)

llama.cpp stops only when the client it is writing to goes away. Two separate
mistakes each looked like working cancellation while a full engine slot kept
decoding to its token limit:

1. `await response.aclose()` inside cleanup that runs while the task is already
   cancelled returns without doing anything. The close is now scheduled as an
   independent shielded task, with a synchronous fallback in `_retire`.
2. `response.aclose()` alone hands the socket back to httpx's pool **still
   open**. Each generation now owns its own `AsyncClient`, and closing that
   client is what closes the socket.

Both were found by watching `GET /slots` on the real engine after a disconnect —
not by any unit test, and not by the daemon's own telemetry, which correctly
reported `queueDepth: 0` while the GPU kept working. Measured after the fix:
engine released 0.6 s after both client disconnect and `POST /v1/responses/{id}/cancel`.

## Gates

Deterministic (`tests/test_muse_llama_cpp.py`, 40 tests, no weights): chat SSE →
`ModelEvent` mapping, inline-`<think>` splitting across chunk boundaries, tool
call reassembly and kind restoration, unknown-tool and bad-argument fail-closed,
engine loading/unreachable/no-chat-surface errors, queue saturation, admission
slot reopening, connection close on cancel *and* on completion, identity
normalization, health fail-closed, both surfaces end to end, control-plane state.

Live (2026-08-10, real weights, engine on a scratch port):

| Gate | Result |
| --- | --- |
| G2 Responses + Chat, stream + non-stream | pass |
| G4 tool call + result round trip over Responses | pass (`{"cmd":"ls"}` → answer) |
| B7 cancel/disconnect stops the engine | pass, 0.6 s both paths |
| B8 usage + cache counts from the engine | pass (61/61, 49 cached) |
| B10 no chain of thought in assistant text | pass |
| F4 health truthful for the active selection | pass |
| E5 compact limit derived from Muse's 131,072 ctx | pass (117,964) |

| G5 installed app, cold start | pass — app spawns the engine, daemon reaches `ready`, live turn answers |

Not yet run: G3 mixed soak, the G5 half that switches Muse ↔ Laguna XS through
the picker, G6 performance report.

### Known engine behavior

`reasoning: {effort: "none"}` / `enable_thinking: false` does **not** suppress
thinking on this template — verified directly against the engine, including via
`chat_template_kwargs` and `reasoning_budget: 0`. Muse thinks on every turn and
the split is honest; a turn with a small `max_output_tokens` can therefore end
`incomplete` inside reasoning. The daemon passes the request through unchanged
rather than pretending the knob works.

## Remaining work

1. G3 mixed soak (interleaved Chat/Responses, switch XS ↔ Muse, unload/reload).
2. Picker round trip in the installed app: pick Muse → Codex turn → transcript
   text; pick Laguna XS → Muse engine stopped, ~20 GB returned.
3. Inference rail: confirm it reads Muse metrics or hides with an explicit
   "unavailable for this runtime" (D2) — the daemon side reports Muse correctly.
4. Vision: only expose when Chat/Responses accept image parts end to end.
5. Nothing here is committed; the working tree also carries another agent's
   changes to `package.json`, `polish.md`, and `tests/bombadil/run.mjs`.

### One residency lie fixed on the way

The sidebar showed "Next free · Frees at 12:29 PM · in 14m 39s" for Muse. The
daemon correctly reports `freeAt: null` — nothing is scheduled, because these
weights are not this daemon's to evict — but `LocalModelResidency` derived a
countdown from the idle setting whenever `freeAt` was absent. It now trusts
`freeAt` and says "Automatic freeing disabled" instead of promising a free that
never comes.

## The bug behind the red sidebar string

`start Muse Glimmer 4-bit llama.cpp Metal engine` was the `anyhow` context of
`Command::spawn`. Root cause: `spawn_muse_engine` executed
`<workshop_root>/scripts/muse/serve.sh`, and the Tauri bundle ships only
`services/laguna-daemon/laguna_daemon` and `visuals` — there is no `scripts/`
directory in `/Applications/Synth Desktop.app`, so the spawn failed with ENOENT
on every installed build. Runtime and weights were both fine.

Fixed by spawning `llama-server` directly from Rust with the arguments inline
(no script dependency), preceded by a preflight that distinguishes:

| phase | shown to the user |
| --- | --- |
| `runtime_missing` | llama.cpp runtime not installed → repair from Settings → Models |
| `weights_missing` | names the missing `.gguf` and its directory |
| `log_unwritable` / `pid_unwritable` | the path and the OS error |
| `spawn_failed` | the binary path and the OS error |

The sidebar also truncated that line to one row with a mid-word ellipsis;
error and warning rows now wrap to three lines with the full text on hover.

## Running more than one Desktop at a time

Instances used to share one Laguna daemon, one engine, one api key, and one
data directory: `desktop-instance.sh` defaulted `SYNTH_LAGUNA_HOME` to
`~/.synth-desktop/laguna` and the base URL to `:7333`. Whichever instance
launched last owned the daemon for everyone, with *its* binary's environment —
which is how an older build bound Muse to the Responses passthrough and made
every local turn in a newer instance fail with "stream disconnected before
completion" while the daemon's own log showed only health polls.

Each instance now derives a stable port pair from its name, beside the Vite
port it already derived:

```text
CHECKSUM   = cksum(instance name)
LAGUNA_PORT = 17300 + (CHECKSUM % 300) * 2
MUSE_PORT   = LAGUNA_PORT + 1
```

and gets its own `SYNTH_LAGUNA_HOME` under the instance's data root, so api
key, pid files, response store, logs, and selected model no longer collide.
Weights stay shared — they are read-only and large.

- The Rust supervisor reads `SYNTH_LAGUNA_PORT` / `SYNTH_MUSE_PORT` per call
  and passes the resolved port to the daemon; the canonical app still defaults
  to 7333/7334.
- Codex is pointed at the base URL the supervisor reported, not a hardcoded
  `:7333`.
- Adopting an already-running engine now matches on the model *and* this
  instance's port, so one Desktop cannot take over another's engine slots.
- The launcher refuses to start when its Laguna port is already held, naming
  the pid, instead of silently sharing.

`scripts/desktop.sh install` builds the working tree, so a shared checkout can
install an app carrying another agent's uncommitted changes. Check `git status`
before treating an installed build as "dev plus my work".

## Key touchpoints

| Need | Location |
| --- | --- |
| Muse backend | `services/laguna-daemon/laguna_daemon/responses_api/backends/llama_cpp.py` |
| Backend selection | `…/responses_api/service.py` (`_make_backend`) |
| Shared tool-call construction | `…/backends/tool_events.py` |
| Shared reasoning classifier | `…/backends/reasoning.py` |
| Identity, engine URL, context length | `…/laguna_daemon/config.py` |
| Health / model card | `…/laguna_daemon/app.py` |
| Engine spawn, daemon env, status text | `apps/synth_desktop/src-tauri/src/laguna.rs` |
| By-hand engine | `scripts/muse/serve.sh` |
| Desktop model identity + compact limit | `runtime/nativeCodex.ts`, `preferences/schema.ts` |
| Sidebar error presentation | `components/ModelDownloadBar.tsx`, `styles/app.css` |
| Tests | `services/laguna-daemon/tests/test_muse_llama_cpp.py` |

## Out of scope

- Training / LoRA on Muse (see `local_lora.md`).
- Shipping weights inside the Desktop DMG.
- Making Codex speak llama.cpp directly.
