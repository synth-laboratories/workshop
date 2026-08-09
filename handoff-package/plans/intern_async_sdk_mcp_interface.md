# Async Intern — SDK / MCP Interface Clarity

**Date:** 2026-07-31  
**Status:** Binding for Async client adapters  
**Supersedes:** multi-`async-assignment` CRUD as the primary SDK/MCP UX  
  (see older §3 in `intern_interaction_boundaries.md` — updated to match this)

**Companions:** `intern_async_mcp_queue_model.md`, `intern_product_positioning.md`,
`intern_runtime_precision_and_a7_lessons.md`

> **Implementation delta (2026-07-31):** this document is the target product
> interface, not the complete current wire surface. Today the backend uses
> `POST /async` for ensure and `POST /async/commands` for writes; REST events
> return a bare array. Operator admission currently permits only `pause`,
> `resume`, `cancel`, and `provide_input`. `message`, `intervene`,
> `redirect_objective`, and `request_checkpoint` still require backend reducer
> semantics before an SDK may advertise them. See
> `intern_remaining_work_handoff.md` for the exact build order. In Python, use
> `client.intern.async_`; `client.intern.async` is invalid syntax.

---

## 1. What you are talking to

```text
Org API key / MCP auth
        │
        ▼
one Research Intern (org)  →  one Async instance (nonstop)
        │
        ├── durable inbox   (commands / messages in)
        ├── event log       (ordered out)
        └── projection      (status, wake, checkpoint, budget, cursor, generation)
```

There is **no** `async_assignment_id` in the v1 client path. The Async Intern
is addressed by **org context alone** (resolved from auth → `org_id` → Intern).

Product feel: tag your Intern (Claude Tag–like). Interface feel: **message queue**.

---

## 2. Layering (one write path)

```text
MCP tools  ──┐
             ├──► Async Intern application service ──► packages/intern reducer
SDK methods ─┘              │
                            ▼
                     Postgres (inbox + events + projection)
                            │
                            ▼
                     InternAsyncWorkflow (Temporal; one per org Intern)
```

| Layer | Role |
|---|---|
| MCP | Ergonomic tools for agents / hosts (Cursor, Claude, etc.) |
| SDK | Same ops as typed methods for scripts / services |
| HTTP | Canonical transport under both (`/smr/research-intern/async/...`) |
| App service | Auth, idempotency, enqueue, projection, event pages |
| `packages/intern` | Pure decisions — only legal writer of Async status vocabulary |

**Parity law:** MCP tool ≡ SDK method ≡ HTTP op ≡ same command envelope /
receipt / events. Ergonomics may differ; **canonical state must not.**

FE Async desk (later) is another producer/consumer on this **same** mailbox.

---

## 3. Queue operations (the whole external interface)

Only four verbs matter:

| Verb | Meaning |
|---|---|
| **Ensure** | Get-or-create org Intern + ensure Async instance/workflow is up; return projection |
| **Send** | Enqueue a typed message/command → receipt (ack, not answer) |
| **Read** | Page events after cursor / sequence |
| **Watch** | Optional long-poll / stream after cursor (not required for progress) |

Control (`pause` / `resume` / `cancel`) is still **Send** with a typed `kind`.

```text
client                         Async Intern
──────                         ────────────
ensure()                  →    projection
send(message|control)     →    InternRuntimeCommandReceipt
events(after=cursor)      ←    EventPage
watch(after=cursor)       ←    EventStream   (optional)
```

Disconnecting MCP/SDK **never** pauses or stops the Intern. Only an explicit
`pause` / `cancel` send does.

---

## 4. HTTP (target canonical surface)

```text
POST /smr/research-intern/async/ensure          # get-or-create + ensure workflow
GET  /smr/research-intern/async                 # projection
POST /smr/research-intern/async/messages        # enqueue (send)
GET  /smr/research-intern/async/events          # ?after_sequence=&limit=
GET  /smr/research-intern/async/events/stream   # optional SSE after cursor
```

No `{assignment_id}` path segment. Org scoping is from auth.

OpenAPI may alias names; semantics stay **one mailbox per org**.

---

## 5. MCP tools (v1)

Thin wrappers. No policy in the tool layer.

| Tool | HTTP | Returns |
|---|---|---|
| `intern_async_ensure` | `POST .../async/ensure` | projection (+ intern id) |
| `intern_async_get` | `GET .../async` | projection |
| `intern_async_send` | `POST .../async/messages` | `InternRuntimeCommandReceipt` |
| `intern_async_events` | `GET .../async/events` | event page + next cursor |
| `intern_async_tail` | `GET .../async/events/stream` or long-poll | events after cursor |

Convenience tools (still same send path):

| Tool | Equivalent send `kind` |
|---|---|
| `intern_async_pause` | `pause` |
| `intern_async_resume` | `resume` |
| `intern_async_intervene` | `intervene` |
| `intern_async_redirect` | `redirect_objective` |
| `intern_async_cancel` | `cancel` |

Optional: `intern_async_get_evidence` — linked read by refs from events
(not a second write path).

### 5.1 `intern_async_send` args

```json
{
  "kind": "message",
  "body": "Keep Factory F healthy; chase blocker on effort E",
  "command_id": "uuid-client-generated",
  "idempotency_key": "stable-retry-key",
  "expected_generation": 12,
  "payload": {}
}
```

| Field | Required | Notes |
|---|---|---|
| `kind` | yes | see §7 |
| `body` | for message/intervene/redirect | human/agent instruction text |
| `command_id` | yes | client-generated UUID; stable on retry |
| `idempotency_key` | yes | dedupe namespace shared with FE/SDK |
| `expected_generation` | yes* | from last projection; omit only if server allows “any” with conflict risk documented |
| `payload` | no | typed extras per kind |

\* Prefer required. Conflict → receipt `conflict` + current generation; client refreshes projection and retries.

### 5.2 Tool result contract

- **Send** → receipt only. Never invent an NL “Intern said…” in the tool layer.
- **Get / ensure** → projection snapshot.
- **Events** → ledger rows. Agent NL replies appear as events
  (`agent_message` / progress / checkpoint / blocker), not as the HTTP body of send.

---

## 6. SDK surface (v1)

Mirror of MCP — same names, typed:

```text
client.intern.async_.ensure() -> AsyncProjection
client.intern.async_.get() -> AsyncProjection
client.intern.async_.send(AsyncMessage) -> InternRuntimeCommandReceipt
client.intern.async_.events(after_sequence: int, limit: int = 100) -> EventPage
client.intern.async_.tail(after_sequence: int) -> AsyncIterator[Event]   # optional

# ergonomic aliases (still send under the hood)
client.intern.async_.pause(...)
client.intern.async_.resume(...)
client.intern.async_.intervene(body=..., ...)
client.intern.async_.redirect(objective=..., ...)
client.intern.async_.cancel(...)
```

Helpers allowed:

```text
client.intern.async_.wait_until(
    predicate,           # e.g. checkpoint advanced / status == paused
    after_sequence=...,
    timeout=...
)   # poll get+events; does NOT hold Temporal open
```

SDK must use the **same** idempotency namespace as MCP/FE.

---

## 7. Message `kind` vocabulary (Async v1)

| kind | Role |
|---|---|
| `message` | Primary “tag the Intern” instruction / objective text |
| `intervene` | Mid-flight steer (same inbox) |
| `redirect_objective` | Explicit objective revision |
| `answer_interaction` | Reply to Intern ask / approval |
| `request_checkpoint` | Ask for product checkpoint |
| `pause` | Fence new Intern effects |
| `resume` | Unfence |
| `cancel` / `stop` | Terminal stop of Async work (product-defined; instance may remain for later ensure) |

Frozen with OpenAPI when implemented. Adding kinds requires doc + OpenAPI amend.

---

## 8. Receipts vs answers (critical)

```text
send()  →  InternRuntimeCommandReceipt
             status: received | delivered | decision | applied
                     | noop | refused | superseded | conflict
```

| Mistake | Correct |
|---|---|
| Treat receipt as Intern finished the research | Receipt = inbox accept / apply decision only |
| Wait on MCP connection for progress | Read `events` / `get` later; Intern keeps working |
| Use heartbeat as progress | Only ledger events + projection fields |
| Double-send without idempotency | Same `idempotency_key` → same receipt |

Progress, NL updates, checkpoints, blockers, evidence refs → **events** (+
projection fields like `next_wake_at`, `checkpoint`, `blocker`,
`evidence_readiness`).

---

## 9. Projection (what `get` / `ensure` return)

Minimum fields:

```text
intern_id
org_id
status                    # running | paused | blocked | ...
state_generation          # CAS / expected_generation
event_cursor              # last sequence (or high-water)
next_wake_at
checkpoint                # product checkpoint summary / id
budget                    # remaining / caps
blocker                   # nullable
leave_safe                # true once durable enough to disconnect
evidence_readiness        # pending | ready | finalizing | ...
external_execution_status # do not treat terminal as evidence ready
cycle_number              # optional
```

Client rule: store `state_generation` + `event_cursor` locally; pass generation
on send; page events after cursor.

---

## 10. Auth

- SDK: org API key (or existing SMR machine auth) → `org_id` → Intern.
- MCP: host auth that resolves to the same org identity.
- No Intern-only credential scheme.
- Same capability / budget gates as FE writes to this mailbox.

---

## 11. What this interface is not

| Not | Why |
|---|---|
| Factory / Run CRUD MCP | Bypass Intern ledger & capability |
| Temporal query surface | Temporal is orchestration, not product state |
| Sync session tools | Sync is a different runtime (many sessions) |
| Internal sandbox agent MCP | Tools *inside* a cycle ≠ external client MCP |
| manderqueue as client API | Actors only; client queue is PG inbox/events |
| Many Async instances | One nonstop Async Intern per org |

---

## 12. Minimal client loops

### Tag and leave (happy path)

```text
ensure()
send(kind=message, body="...", idempotency_key=K)
# disconnect — Intern continues
# later:
get()
events(after=cursor)   # checkpoints, progress, asks
```

### Intervene while running

```text
p = get()
send(kind=intervene, body="...", expected_generation=p.state_generation, idempotency_key=K2)
events(after=...)
```

### Pause

```text
send(kind=pause, ...)
get()  # status=paused; no new cycles until resume
```

### Idempotent retry

```text
send(..., idempotency_key=K)   # network timeout
send(..., idempotency_key=K)   # same receipt; no double apply
```

---

## 13. Done when

One SDK script + one MCP tool sequence each prove:

1. `ensure` → projection with `leave_safe` (or clear provisioning then leave_safe).  
2. `send(message)` → receipt; disconnect; later `events` show ≥1 checkpoint/progress.  
3. Duplicate `idempotency_key` → identical receipt; one apply.  
4. `pause` → projection paused; no new cycle until `resume`.  
5. Same org mailbox visible to FE Async desk (when built) — same events/generation.

---

## 14. Relation to older “async-assignments” OpenAPI

Existing OpenAPI `async-assignments` paths are **legacy relative to this
interface**. Implementation should either:

1. **Replace** with `/async` mailbox routes above, or  
2. **Collapse** so `async-assignments` is a singleton (get-or-create the one
   org Async instance) and hide multi-create from MCP/SDK.

Do not ship MCP tools that require clients to create N assignments.
