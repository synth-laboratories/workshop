# QA handoff — Muse Glimmer as a local model

**What changed:** Muse Glimmer (30B GGUF, llama.cpp + Metal) is now served by
the Laguna sidecar the same way Laguna XS is. Both `/v1/responses` and
`/v1/chat/completions` work on `127.0.0.1:7333`, streaming and not, with tools,
reasoning, usage, and cancellation. Before this, every Muse turn failed.

**Branch:** `dev`, commits `04b56b8`, `e806013`, `1058417` (unpushed at time of
writing). Design notes and the full contract: `apps/synth_desktop/muse_sidecar.md`.

---

## Read this first: only one local model runtime is permitted

Instances used to share one daemon on one port. Whichever launched last owned it
for everyone — with *its* binary's environment. That is not a hypothetical: an
older instance bound Muse to the wrong backend and every turn in a newer
instance failed with **"The provider could not produce a response: stream
disconnected before completion"**, while that instance's own daemon log showed
nothing but health polls.

Instances now get their own port pair and data directory, and Synth takes a
machine-wide advisory lease before starting any local model. A second app may
run for cloud/UI work, but its local inference remains disabled until the owner
is closed or unloaded. This is deliberate: ports isolate requests, not Apple's
unified memory, and parallel Muse/Laguna loads can exhaust the machine.

For QA:

```bash
ps -axo pid=,command= | grep "Synth Desktop.*MacOS" | grep -v grep
```

If more than one Desktop is running, check which one owns local inference. The
owner is recorded in `~/.synth-desktop/local-model-runtime.lock`; the kernel
releases the lease automatically if its app crashes. Named development
instances also detect older runtimes that predate the lease and launch with
local auto-start disabled.

Each instance remains request-isolated — check which port yours is on:

```bash
./scripts/desktop-instance.sh status <name>     # prints its laguna + muse ports
```

The canonical `/Applications` app stays on `7333` (daemon) and `7334` (engine).
Everything below assumes the canonical app.

---

## Setup

1. Install the build under test: `./scripts/desktop.sh install`
   - It builds the **working tree**. On a shared checkout run `git status`
     first — otherwise you may install someone's uncommitted work and report
     their bug as ours.
2. Weights (~20 GB total) in `~/.synth-desktop/models/meta-models/Muse-Glimmer-30B-GGUF/`:
   `muse-glimmer-30B-kquant-17gb.gguf`, `mmproj-kquant.gguf`, `dflash-kquant.gguf`.
   Missing? Settings → Models → download.
3. Runtime at `~/.synth-desktop/muse/runtime/llama-dd1ea52.../llama-server`.
   Missing? Settings → Models → repair.
4. Select **Muse Glimmer 30B** in the launch picker / Composer model menu.

Useful shell handles:

```bash
KEY=$(tr -d '\n' < ~/.synth-desktop/laguna/api_key)
H="Authorization: Bearer $KEY"
curl -s -H "$H" http://127.0.0.1:7333/health | python3 -m json.tool
```

Logs: daemon `~/.synth-desktop/laguna/desktop-sidecar.log` ·
engine `~/.synth-desktop/laguna/muse-llama.log`.

---

## What "working" looks like

`/health` with Muse selected:

```json
{ "status": "ok", "responsesApi": true, "chatCompletionsApi": true,
  "backend": "llama_cpp", "defaultModel": "meta-models/Muse-Glimmer-30B-GGUF",
  "memoryBytes": null, "freeAt": null,
  "engine": { "state": "ready", "detail": "The Muse engine is serving." } }
```

`memoryBytes: null` and `freeAt: null` are **correct, not bugs** — see
"Expected behavior that looks wrong" below.

---

## Test pass

### A. Cold start (the one that was broken)

1. Quit Desktop. Confirm nothing is left:
   `lsof -nP -iTCP:7333 -iTCP:7334 | grep LISTEN` → empty.
2. Launch Desktop with Muse selected.
3. **Expect:** sidebar shows Muse Glimmer, no red text. Engine appears within
   ~60 s cold (`lsof -nP -iTCP:7334`). Health reaches `ok` / `engine: ready`.
4. Send "hey". **Expect:** a reply, no error.

**Regression watch:** a red `start Muse Glimmer 4-bit llama.cpp Metal engine`
means the old spawn path is back. Any failure should now name the cause —
missing runtime, missing weights, a port in use — not the step.

### B. Both wire surfaces

```bash
# Responses (what Codex uses)
curl -s -H "$H" -H 'Content-Type: application/json' \
  http://127.0.0.1:7333/v1/responses \
  -d '{"model":"meta-models/Muse-Glimmer-30B-GGUF","input":"Reply with exactly: pong","store":false,"max_output_tokens":500}'

# Chat Completions (must NOT be a 501)
curl -s -H "$H" -H 'Content-Type: application/json' \
  http://127.0.0.1:7333/v1/chat/completions \
  -d '{"model":"meta-models/Muse-Glimmer-30B-GGUF","messages":[{"role":"user","content":"Reply with exactly: pong"}],"max_tokens":500}'
```

Add `"stream": true` to each and confirm frames arrive and end with
`data: [DONE]`. A 501 on Chat, or a 502 on Responses, is the old passthrough
returning.

### C. Reasoning never leaks

Muse thinks on **every** turn. In the transcript, thinking must appear as
reasoning, never inside the assistant message. Grep any raw response for
`<think>` — finding it in `content` / `output_text` is a bug.

### D. Tools (the Codex path)

Ask it to run a command ("list the files in this directory"). Expect a real
tool call, execution, and an answer that uses the output. Multi-step edits and
`apply_patch` are worth exercising here too.

### E. Cancel

Ask for something long ("count slowly from 1 to 400"), then press Stop.

```bash
curl -s -H "$H" http://127.0.0.1:7334/slots | python3 -c "import json,sys; print([s['id'] for s in json.load(sys.stdin) if s.get('is_processing')])"
```

**Expect:** empty within ~1 s. If a slot keeps processing, the GPU is burning
on a turn nobody wants — file it, this exact bug existed and was fixed. The
daemon reporting `queueDepth: 0` is **not** sufficient evidence; check the
engine.

Same check after closing a tab or quitting mid-turn.

### F. Switching models

Muse → Laguna XS: the engine must stop and ~20 GB return
(`lsof -nP -iTCP:7334` empty). Then a Laguna turn must work. Switch back.

### G. Long context

Muse's window is 131,072 tokens (vs 262,144 for XS) and compaction is set to
100,000 by default (Settings → General → Agent context). Run a long session and
confirm compaction happens without a stuck or looping turn.

---

## Expected behavior that looks wrong

| You see | Why it is correct |
| --- | --- |
| `Memory unavailable` instead of GB resident | The weights live in the engine process; this daemon has no allocator counter for them. Reporting the file size on disk would be a filesystem fact labeled as memory. |
| No "frees at …" countdown; "Automatic freeing disabled" | Muse stays resident while selected. The daemon cannot unload another process's weights, and a countdown would promise a free that never comes. Memory returns on model switch or quit. |
| Muse thinks even with reasoning set to none | The template ignores every thinking-off knob (`enable_thinking`, `chat_template_kwargs`, `reasoning_budget: 0`) — verified against the engine directly. Pass-through is deliberate; we do not pretend the knob works. |
| A short `max_output_tokens` ends "incomplete" | It ran out of budget while still reasoning. Raise the limit. |
| Vision is not offered | The projector is loaded, but no surface sends image parts to the engine yet, so the capability bit stays false on purpose. |
| Unload returns 409 `engine_release_not_supported` | The engine's lifetime belongs to the Desktop supervisor. Deselect Muse to release. |

---

## Filing a bug

Include:

1. `curl -s -H "$H" http://127.0.0.1:7333/health` (whole body).
2. The tail of both logs (daemon and engine).
3. `ps -axo pid=,command= | grep "Synth Desktop.*MacOS" | grep -v grep` —
   proves whether a second instance was involved.
4. `curl -s -H "$H" http://127.0.0.1:7334/slots` if it is a hang or a cancel.
5. Whether the same thing happens on Laguna XS. Only Muse means the backend;
   both means the shared turn core.

**Triage shortcut:** if `responses.backend` in `/health` is anything other than
`LlamaCppChatBackend` while Muse is selected, stop — the daemon is bound wrong,
which almost always means a second instance or a stale build owns the port.

---

## Covered in the 2026-08-10 QA pass

- Mixed concurrent soak across Chat and Responses.
- Muse → Laguna → Muse switching and memory release.
- TTFT/decode/request telemetry and Muse identity in the inference rail.
- Cancellation and slot release, including a disconnected client.
- 8K-token prompt processing, exact artifact-size validation, missing-runtime
  guidance, isolated ports, forced dev shutdown, and the machine-wide local
  runtime admission guard.
