# synth-local-runtime

A loopback-only, standard-library Python daemon that owns Synth Desktop sessions,
runs, event replay, local model adaptation, and Intern mailbox adaptation.

## Run directly

```bash
cd ../../
PYTHONPATH=services/local-runtime/src \
  python3 -m synth_local_runtime --port 8765
```

The daemon defaults to:

- SQLite WAL at `~/.synth-desktop/runtime/runtime.sqlite3`
- local Laguna deterministic streaming stub
- explicit Intern demo mode when no `SYNTH_API_KEY` is present
- `127.0.0.1` only

Electron normally starts the daemon on an ephemeral port and places a private
connection descriptor at `~/.synth-desktop/runtime/connection.json`. It deliberately
leaves the daemon alive when the window closes, preserving Async supervision and
cursor replay.

## API

```text
GET    /v1/health
GET    /v1/sessions
POST   /v1/sessions
GET    /v1/sessions/{id}
DELETE /v1/sessions/{id}
POST   /v1/sessions/{id}/messages
POST   /v1/sessions/{id}/commands
GET    /v1/sessions/{id}/runs
GET    /v1/sessions/{id}/events?after_sequence=N
GET    /v1/sessions/{id}/events/stream?after_sequence=N
POST   /v1/shutdown
```

The SSE cursor is the **desktop event sequence**. Intern’s server sequence is
stored separately as `(session_id, source=intern, cursor)` and is never mixed
with local sequences.
