# API Cheat Sheet — Intern Sync / Async

Base path: `/smr/research-intern`  
Auth: `Authorization: Bearer <org API key>` (SDK/MCP/desktop daemon)  
Web FE today uses Clerk → Next BFF `/api/smr/...` — **Electron must not**.

Parity law: **MCP ≡ SDK ≡ HTTP ≡ same mailbox**.

---

## Sync (many sessions)

| Method | Path | Notes |
|--------|------|-------|
| `POST` | `/sync-sessions` | Create; body `SyncSessionCreateRequest` → 202 |
| `GET` | `/sync-sessions` | List |
| `GET` | `/sync-sessions/{id}` | Projection |
| `POST` | `/sync-sessions/{id}/commands` | Generation-fenced command |
| `GET` | `/runtimes/sync/{id}/events?after_sequence=&limit=` | Page ≤500 |
| `GET` | `/runtimes/sync/{id}/events/stream` | SSE; `Last-Event-ID` |
| `PUT` | `/sync-sessions/{id}/presence` | Presence lease (approvals) |

### Create body (shape)

```json
{
  "objective": "",
  "idempotency_key": "…",
  "binding": {
    "factory_id": null,
    "project_id": null,
    "effort_id": null,
    "run_id": null
  },
  "metadata": {},
  "execution_mode": "standard",
  "require_operator_approval": true
}
```

### Command envelope

```json
{
  "command_id": "uuid",
  "idempotency_key": "uuid",
  "expected_generation": 0,
  "command_kind": "operator_message",
  "payload": { "body": "…", "context": {}, "turn_id": null }
}
```

### Sync command kinds (admitted)

`submit_turn` · `operator_message` · `intervene` · `answer_interaction` · `pause` · `resume` · `close`

### Receipt statuses

`received` | `delivered` | `applied` | `noop` | `refused` | `superseded` | `conflict`

HTTP 202 + receipt ≠ “agent finished”. Wait for ledger events.

---

## Async (org singleton)

| Method | Path | Notes |
|--------|------|-------|
| `POST` | `/async` or `/async/ensure` | Ensure singleton + workflow |
| `GET` | `/async` | Projection |
| `POST` | `/async/commands` | Control + instructions |
| `POST` | `/async/messages` | Typed message alias |
| `GET` | `/async/events?after_sequence=` | Page |
| `GET` | `/async/events/stream` | SSE |

**No runtime id in path** — one Async Intern per org.

### Async command / instruction kinds

`pause` · `resume` · `cancel` · `provide_input` · `answer_interaction` · `message` · `intervene` · `redirect_objective` · `request_checkpoint` · `request_spine_handoff` · `advance_spine`

### Critical UX rule

**Disconnect ≠ pause.** Closing Desktop must leave Async running.

---

## SSE contract

```text
id: {sequence}
event: {event_kind}
data: {"schema_version":"smr.intern-runtime-event-stream.v1","event":{...}}
```

- Resume: `?after_sequence=N` and/or `Last-Event-ID: N` (max of both)
- Heartbeat: `: heartbeat` ~15s — **not** progress
- Authority: Postgres; Redis wake is hint-only

Frontend reference implementation: `excerpts/frontend/researchIntern.ts`  
(`listInternRuntimeEvents`, `openInternRuntimeEventStream`)

Backend: `excerpts/backend/runtime_events.py`

---

## SDK (Python) — prefer in daemon

```python
from synth_ai import SynthClient

c = SynthClient(api_key=..., base_url=...)  # or env SYNTH_API_KEY / SYNTH_BACKEND_URL
sync = c.research.intern.sync_
async_rt = c.research.intern.async_

session = sync.create(...)
sync.send_message(session.sync_session_id, command_id=..., idempotency_key=...,
                  expected_generation=session.state_generation, body="...")
page = sync.tail(session.sync_session_id, after_sequence=0, event_count_max=50)
# persist page.next_sequence

rt = async_rt.ensure(...)
async_rt.send(...)
page = async_rt.events(after_sequence=0)
```

Attribute names: **`sync_` / `async_`** (trailing underscore).

MCP mirrors: `intern_sync_*`, `intern_async_*` via `synth-ai-research-mcp`.

---

## Local Pilot (optional later)

Loopback-only endpoints under research-intern for attach/context/commands/detach.  
Gated by `INTERN_LOCAL_PILOT_ENABLED`. See `excerpts/backend/local_pilot.py`.  
Desktop can eventually **be** the pilot host for Sync turns.

---

## MCP ↔ HTTP (client tools)

| MCP | HTTP |
|-----|------|
| `intern_sync_create` | `POST .../sync-sessions` |
| `intern_sync_send` | `POST .../sync-sessions/{id}/commands` |
| `intern_sync_events` / `tail` | events / stream |
| `intern_async_ensure` | `POST .../async/ensure` |
| `intern_async_send` | `POST .../async/messages` |
| `intern_async_events` / `tail` | events / stream |

In-cycle agent tools are `smr_*` — not the desktop control plane.

---

## OpenAPI

Machine contract: [`../contracts/research-v1.json`](../contracts/research-v1.json)

Generate Electron/renderer types from this file. Keep lockstep with backend `openapi_contract.py` allowlist.

---

## Internal Codex activity (do not use as product API)

`GET /smr/internal/codex-activity/runs/{run_id}/stream` — worker token, evidence only.  
See `references/CODEX_ACTIVITY_STREAM_HANDOFF.md`.
