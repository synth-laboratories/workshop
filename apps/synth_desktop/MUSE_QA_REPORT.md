# Muse Glimmer QA report — 2026-08-10

Branch: `codex/muse-glimmer-qa`  
Worktree: `/Users/joshuapurtell/Documents/GitHub/workshop-muse-qa`

## Result

Muse Glimmer's GGUF + DFlash inference path works through both supported API
surfaces. QA found and fixed three release-blocking lifecycle/safety defects:

1. quitting a development instance could leave its detached daemon and Muse
   engine resident;
2. the Tauri watcher could respawn an app immediately after `stop`;
3. isolated app deployments could each load another 20–30 GiB local model and
   exhaust unified memory.

The fix enforces one Synth-managed local runtime per Mac with an advisory file
lock, releases it on normal shutdown/failure, and starts secondary development
instances with local inference disabled when an older runtime is detected.
The sidebar now preserves that reason rather than saying "sidecar not
connected."

## Live coverage

| Area | Result |
| --- | --- |
| Cold Muse start | Pass; engine and daemon ready, canonical Muse identity |
| Responses, non-streaming | Pass; HTTP 200, assistant text, no reasoning leak |
| Responses, streaming | Pass; SSE frames and `[DONE]` |
| Chat Completions, non-streaming | Pass; HTTP 200, separated reasoning/content |
| Chat Completions, streaming | Pass; SSE frames and `[DONE]` |
| Tool calling | Pass; typed `exec_command` call with valid JSON arguments |
| Cancellation | Pass; abandoned stream released its llama.cpp slot |
| DFlash | Pass; live draft acceptance observed at roughly 50–70%, including 93.6% on the 8K prompt |
| Mixed soak | Pass on repeat: six concurrent interleaved Chat/Responses requests, all HTTP 200 with exact outputs |
| Inference telemetry | Pass; Muse identity, queue 0/9, TTFT/decode/request metrics; no Laguna leakage |
| Model switch | Pass; Muse → Laguna stopped the GGUF engine, Laguna returned `laguna-ok`, Muse restarted correctly |
| Long prompt | Pass; 8,076 prompt tokens processed without truncation at about 127 prompt tokens/s |
| Port isolation | Pass; canonical 7333/7334 and QA 17726/17727 did not share traffic |
| Forced dev stop | Pass after fix; app watcher, Vite, daemon, engine, ports, and PID files all cleared |
| Concurrent-runtime admission | Pass; held machine lease prevented a second QA daemon/engine from opening |
| Legacy-runtime dev guard | Pass; active pre-lease Muse/Laguna processes disabled QA local auto-start |
| Missing artifacts/runtime | Pass; actionable Settings → Models repair guidance |
| Corrupt/partial artifacts | Pass; exact pinned sizes accepted and one-byte-short DFlash rejected |
| Vision capability | Corrected; UI now advertises text only because the current API does not send image parts |

The 8K request's client process disconnected before collecting its response,
but the engine completed prompt evaluation and generation, released the slot,
and reported `truncated = 0`. Context configuration remains 131,072 tokens.

## Automated coverage

- Laguna daemon Muse suite: **40 passed**.
- Rust Laguna suite: **28 passed**.
- Frontend production build: **passed**.
- Shell syntax and `git diff --check`: **passed**.
- TypeScript typecheck: blocked by pre-existing `dev` integration errors in
  `App.tsx` (`SettingsPage.account` and `TerminalPanel.height` prop contracts),
  unrelated to Muse; the production Vite build succeeds.

## Memory incident and policy

The QA audit ran on a 64 GiB Mac and observed one QA Muse process at about
19.5 GiB RSS while another Muse engine and multiple Laguna daemons existed.
Port isolation alone was therefore unsafe.

New policy:

- `~/.synth-desktop/local-model-runtime.lock` is held for the lifetime of a
  Synth local runtime; the OS releases it on process death.
- A second packaged app gets an actionable error naming the lease owner and
  does not spawn local inference.
- A named dev instance detects both new and older Synth model processes before
  launch and sets `SYNTH_LAGUNA_AUTO_START=0` when another runtime is active.
- `desktop-instance.sh stop` terminates the watcher before the app, then reaps
  only ownership-validated daemon/engine PIDs.
- Local inference can be restored by closing/unloading the owner and restarting
  the secondary dev instance. There is no automatic eviction of another app's
  model.

## Known non-Muse blocker

The committed `dev` baseline does not currently pass `npm run typecheck` due to
unrelated component-prop drift. It should be resolved before treating the
entire Desktop branch as release-clean.

## Follow-up live deployment failure

A later Desktop screenshot failed with `stream closed before
response.completed`. The window was a dirty `aesthetic-audit` bundle built at
`a94fda7`, before the Muse sidecar fix. It spawned
`RemoteResponsesBackend`/`SYNTH_LAGUNA_BACKEND=external` against a Muse engine
left by another app; the engine rejected its different API key with HTTP 401.
A separate stale `cloudqa` bundle then reclaimed the same canonical port and
home after the first app exited.

After those two pre-fix bundles were stopped, this branch started on its named
17726/17727 ports with `LlamaCppChatBackend`. A live Chat request returned
`muse-works` with HTTP 200, and a streaming Responses request emitted output
text, `response.completed`, and `[DONE]` with `responses-work`.

The transcript now reports local phases explicitly: warming/loading,
queueing, context prefill, and generation. Muse's inference rail is labeled
"Local GGUF engine" rather than "MLX sidecar."
