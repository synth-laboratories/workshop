# Synth Desktop session lifecycle / E2E handoff

Date: 2026-08-09

## Goal

Make local Codex/Laguna conversations survive app-server crashes and desktop quit/relaunch without stale `Working` UI, broken `Stop`, lost threads, stale SQLite runs, or abandoned Laguna generations occupying the MLX queue.

## Architecture now

- SQLite/event journal is the durable source of truth for sessions and runs.
- Codex app-server attachments are ephemeral and generation-fenced.
- A lost app-server changes the active run to `interrupted` with `app_server_exited` and emits `session/unhealthy`.
- Desktop startup reconciles detached active runs with `desktop_restarted`.
- `Stop` is idempotent when the in-memory app-server no longer exists.
- A later message starts a replacement app-server and uses `thread/resume` with the persisted Codex thread ID.
- Laguna native Responses streams now cancel their backend task when the HTTP client disconnects. A custom ASGI streaming response listens for `http.disconnect` even during a long MLX prefill gap.

See also [SESSION_LIFECYCLE.md](SESSION_LIFECYCLE.md).

## Commits

- `f6004f4` — Harden Synth Desktop runtime lifecycle and polish.
- `06837bd` — Add real spawned-process crash/resume lifecycle tests.
- `6143d9d` — Reconcile partially persisted detached runs and cancel abandoned Laguna streams.

Branch is `main`, currently five commits ahead of `origin/main`.

## Bugs found during installed-app acceptance

### 1. App-server crash split brain

Reproduced on the canonical installed app by starting a local Laguna turn, resolving the exact `codex app-server` child of the canonical Synth PID, and terminating only that child.

Observed and confirmed:

- Sidebar and transcript cleared `Working` / `Stop`.
- Transcript showed `Stopped because the local agent disconnected · send a message to reconnect`.
- SQLite run became `interrupted` with `{"reason":"app_server_exited"}`.
- A follow-up created a replacement app-server and retained Codex thread ID `019fe7b9-4dff-7e52-b3a6-df41fd559dc1`.

### 2. Graceful quit could update JSON but not SQLite

The attachment JSON sometimes reached `interrupted` before process exit while SQLite still held the active run as `running`. Startup only repaired SQLite when it also changed the JSON status, so this partially reconciled state survived relaunch.

Fix in `CodexManager::list`: every detached record now attempts durable active-run reconciliation, independently of whether the JSON status changed during that call.

Installed rebuilt-app evidence:

- Session `5dcaba82-f8ad-4cba-8c6e-9779db85d887` became `interrupted`.
- Its stale run became `interrupted` with `{"reason":"desktop_restarted"}`.
- `active_run_id` was cleared.
- Same Codex thread ID remained intact.

### 3. Abandoned Laguna HTTP streams could hold the MLX slot

Killing the app-server could leave its native Responses generation running, causing the replacement turn to queue behind work nobody could receive. The service already cancelled an inner task when its iterator closed, but Starlette's ASGI 2.4 path can wait for a later socket write to notice disconnect; MLX prefill may not yield for tens of seconds.

Fix in `6143d9d`:

- `ResponsesService.stream` accepts a disconnect probe and cancels/gathers the generation task.
- `DisconnectAwareStreamingResponse` always listens for `http.disconnect` and cancels the body iterator immediately.
- Both the service boundary and ASGI boundary have regression tests.

## Tests run and passing

```text
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib
92 passed

npm --prefix apps/synth_desktop run typecheck
passed

PYTHONPATH=. ~/.synth-desktop/laguna/.venv/bin/python -m unittest -v tests.test_native_responses
26 passed
```

Important Rust integration coverage:

- Real fake app-server subprocess is spawned through production `tokio::process::Command` code.
- Forced child exit interrupts the JSON record and SQLite run.
- Detached `Stop` succeeds.
- Replacement uses `thread/resume` with the original thread ID.
- A stale attachment EOF cannot detach its replacement.
- Startup reconciles both `running` JSON records and already-`interrupted` JSON records whose SQLite run is still active.

Important Laguna coverage:

- A service disconnect cancels the backend generation and clears `coordinator.active`.
- ASGI `http.disconnect` cancels a stream while its body is in a simulated 60-second prefill gap.
- Full native Responses test module passes.

## Installed-app E2E evidence

The rebuilt and ad-hoc-signed app was installed at `/Applications/Synth Desktop.app`. The previous app is recoverable at:

```text
~/.synth-desktop/backups/app-builds/Synth Desktop-20260809-1422-pre-e2e.app
```

One durable acceptance chat is titled `Lifecycle acceptance test`.

Confirmed through UI + SQLite:

- Real local Laguna turn entered `Working`.
- Exact owned app-server was terminated.
- UI surfaced disconnected state without a throwing Stop action.
- Run persisted `app_server_exited`.
- Desktop quit/relaunch persisted `desktop_restarted` for an orphaned active run.
- Same thread resumed and completed with `lifecycle resume confirmed` and later `final lifecycle confirmed`.
- Completed run outcome contained the same persisted thread ID.

## What remains

Run one clean, isolated final acceptance of the Laguna disconnect-cancellation patch after restarting the daemon so it loads commit `6143d9d`:

1. Ensure only the canonical installed app is used for UI acceptance. Several similarly named alpha/beta/test instances are running; CUA targeting became ambiguous late in the session.
2. Use a Laguna port not shared by those instances, or temporarily stop only the specifically assigned test instance. Do not disturb alpha/beta without coordination.
3. Restart that Laguna daemon after `6143d9d`; the package is editable from `services/laguna-daemon`, but an already-running Python process keeps the old imported code.
4. Start a long turn, confirm the exact app-server PID by parent chain, and force-kill it.
5. Poll `/health` and require `responses.runtime.inflight_generations == 0`, `queued_generations == 0`, and `generation_slot_available == true` within a short bound.
6. Send a follow-up in the same chat and require a completed response with the unchanged `codex_thread_id`.

The service/ASGI tests cover this behavior, but the last live port-7335 attempt was confounded by another Synth instance using the same daemon. Do not claim the new ASGI cancellation patch has a clean isolated installed-app acceptance until the six steps above pass.

## Current machine state

- Canonical installed app is running from `/Applications/Synth Desktop.app`.
- Canonical Laguna daemon on port `7333` is running, but it started before the Python disconnect patch and must be restarted to load it.
- The temporary port `7335` daemon used during acceptance is no longer running.
- Alpha, beta, and test instances are also running. Preserve them unless their owner explicitly assigns them to the acceptance.
- Never paste or log Laguna/OpenRouter credentials. A process listing can include the Laguna key in argv; redact it from reports.

## Dirty worktree warning

Many unrelated/concurrent edits remain unstaged, including renderer polish, container/visual work, docs, tests, and Laguna MLX changes. They belong to other workstreams. Do not reset, clean, or bulk-stage them. `work/` is large scratch data and must not be committed.

`services/laguna-daemon/tests/test_native_responses.py` remains modified after `6143d9d` because unrelated test/import changes were intentionally left unstaged. The lifecycle commit staged only the two disconnect tests and their `DisconnectAwareStreamingResponse` import.

## Useful commands

```bash
./scripts/desktop.sh status

cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib
npm --prefix apps/synth_desktop run typecheck

cd services/laguna-daemon
PYTHONPATH=. ~/.synth-desktop/laguna/.venv/bin/python -m unittest -v tests.test_native_responses

sqlite3 -header -column \
  "$HOME/Library/Application Support/Synth Desktop/synth.sqlite3" \
  "select id,title,status,codex_thread_id,active_run_id,updated_at from sessions order by updated_at desc limit 10;"
```

For exact process ownership, start with the canonical Synth PID from `./scripts/desktop.sh status`, then inspect only direct children. Never kill a broad name/glob because other Codex and Synth instances are active.
