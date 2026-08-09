# Intern Interaction Boundaries

**Date:** 2026-07-31  
**Status:** Binding — Async↔FE, Async↔MCP/SDK, Sync↔FE  
**Rule:** Same Intern application service for all adapters. No private write APIs.

Companion: `intern_frontend_interaction_spec.md`, `intern_sync_clarity.md`,
`intern_async_clarity.md`.

---

## 0. Shared boundary law (all three)

```text
Adapter (FE | SDK | MCP)
        │
        │  WRITE: InternRuntimeCommandRequest
        │  READ:  projection + events (+ linked SMR GETs)
        │  LIVE:  SSE / stream after cursor   (FE primary; SDK optional)
        ▼
Intern control plane → reducer → Postgres → Temporal → effects
        │
        ▼
InternRuntimeCommandReceipt + ledger events
```

| May cross the boundary | Must not |
|---|---|
| Typed commands with `command_id`, `idempotency_key`, `expected_generation` | Silent DB/state edits |
| Receipts: `received\|delivered\|applied\|noop\|refused\|superseded\|conflict` | Treating HTTP 200 as “Intern finished answering” |
| Projections + ordered events | Heartbeats as progress |
| Linked reads (experiments, visuals, data, runs) | Inventing Intern NL replies client-side |
| | Using manderqueue as the client↔Intern API |

**Auth:** signed-in FE uses the same SMR session/auth path as other `/smr/*`
routes. SDK/MCP use org API keys / MCP auth that resolve to the same `org_id`
→ Research Intern. No Intern-only credential scheme.

---

## 1. Sync ↔ Frontend

**Product:** live, dynamic research workbench in the browser.

### 1.1 Resources

```text
UI:  /smr/intern/sync
     /smr/intern/sync/[sync_session_id]

API: POST/GET /smr/research-intern/sync-sessions
     GET      /smr/research-intern/sync-sessions/{sync_session_id}
     POST     /smr/research-intern/sync-sessions/{sync_session_id}/commands
     GET      .../runtimes/sync/{sync_session_id}/projection
     GET      .../runtimes/sync/{sync_session_id}/events
     GET      .../runtimes/sync/{sync_session_id}/events/stream
```

### 1.2 What FE sends (writes)

| command_kind | When |
|---|---|
| `operator_message` | Chat / research instruction (`payload.body`) |
| `pause` / `resume` | Session control |
| `intervene` | Steer bound Run/Swarm |
| `answer_interaction` | Clarification / approval |
| `close` | End session with typed outcome |

Create session: `POST .../sync-sessions` (not a generic command).  
Every write: client-generated `command_id` + `idempotency_key` +
`expected_generation` from last projection.

### 1.3 What FE receives

| Channel | Content |
|---|---|
| Command HTTP | `InternRuntimeCommandReceipt` only (not NL reply) |
| SSE / events | `operator_message`, `agent_message`, progress, blockers, `InteractionRequested`, `resource_refs` |
| Projection | `status`, `state_generation`, `pending_turn_id`, binding, cursor |
| Linked GETs | **Live panes:** Experiments, Visuals, Data bindings/revisions, Run/actors, costs, evidence — scoped by `RuntimeBinding` |

### 1.4 Live session loop

```text
open Sync page
→ GET projection + backfill events + open SSE
→ user types → POST operator_message
→ show optimistic bubble (command_id)
→ receipt applied/delivered → wait for agent_message event
→ resource_refs on events → refresh Experiments / Visuals / Data panes
→ reload → same sync_session_id + cursor resume
```

### 1.5 Sync↔FE must not

- Use legacy `/sessions/.../turns` as the product write path.  
- Drive Magi five-receipt chain as the Sync stage model.  
- Treat tab close as session cancel (only explicit `close` / pause).  
- Write Experiments/Visuals/Data except via Intern commands or navigating to
  owner pages.

### 1.6 Sync↔FE done when

Playwright `fe_intern_sync_live_reconnect` + `fe_intern_sync_live_surfaces`
(transcript + live Experiments/Visuals/Data) pass against real backend.

---

## 2. Async ↔ Frontend

**Product:** always-on assignment; FE is optional supervisor.

### 2.1 Resources

```text
UI:  /smr/intern/async
     /smr/intern/async/[async_assignment_id]

API: POST/GET /smr/research-intern/async-assignments
     GET      /smr/research-intern/async-assignments/{async_assignment_id}
     POST     /smr/research-intern/async-assignments/{async_assignment_id}/commands
     GET      .../runtimes/async/{async_assignment_id}/projection
     GET      .../runtimes/async/{async_assignment_id}/events
     GET      .../runtimes/async/{async_assignment_id}/events/stream
```

### 2.2 What FE sends (writes)

| command_kind | When |
|---|---|
| `intervene` | Mid-run instruction |
| `pause` / `resume` | Fence / unfence new Intern effects |
| `redirect_objective` | Explicit objective revision |
| `answer_interaction` | Clarification |
| `request_checkpoint` | Ask for product checkpoint |
| `cancel` / `stop` | Terminal stop |

Create: `POST .../async-assignments` → durable id; UI shows **leave_safe**
immediately. If status is provisioning, do not claim cycles yet.

### 2.3 What FE receives

| Channel | Content |
|---|---|
| Command HTTP | Same receipt enum as Sync |
| Projection | `status`, `cycle_number`, `next_wake_at`, `checkpoint`, `budget`, `evidence_readiness`, `external_execution_status`, `blocker`, `leave_safe` |
| Events / SSE | Timeline: wake, cycle, checkpoint, intervene, evidence, blockers (optional while tab open) |
| Linked GETs | Resource detail **on demand** when timeline cites ids — not Sync’s always-on multi-pane workbench |

### 2.4 Leave / return loop

```text
POST create assignment → show id + “you may leave”
→ close tab (runtime continues)
→ return later → GET projection (same async_assignment_id)
→ backfill events / open SSE
→ intervene / pause as commands
→ checkpoint + next_wake from server, never from FE timers
```

**Critical:** FE disconnect ≠ pause ≠ cancel. Only explicit commands change
control state.

### 2.5 Async↔FE must not

- Look like Sync cockpit with mode=async.  
- Require SSE for progress.  
- Poll as the Intern scheduler.  
- Claim complete when `external_execution_status=terminal` but
  `evidence_readiness` still pending/finalizing.

### 2.6 Async↔FE done when

Playwright `fe_intern_async_leave_safe` + backend
`intern_async_leave_cycle_intervene` pass.

---

## 3. Async ↔ MCP / SDK

**Product:** one nonstop Async Intern per org; MCP/SDK = message-queue client.  
**Binding detail:** `intern_async_sdk_mcp_interface.md` (source of truth for this
boundary).

### 3.1 Shape

```text
ensure / get  → projection
send(kind)    → InternRuntimeCommandReceipt   # ack, not answer
events/tail   → ordered ledger after cursor
```

Addressed by org auth alone — **no** `async_assignment_id` in the v1 client path.

### 3.2 Parity law

```text
MCP tool ≡ SDK method ≡ HTTP /async/* ≡ same inbox + receipts + events
```

Ergonomics may differ; **canonical state must not.** FE Async desk (later)
shares the same mailbox.

### 3.3 MCP / SDK verbs (v1)

| MCP | SDK | HTTP |
|---|---|---|
| `intern_async_ensure` | `client.intern.async.ensure()` | `POST .../async/ensure` |
| `intern_async_get` | `.get()` | `GET .../async` |
| `intern_async_send` | `.send(...)` | `POST .../async/messages` |
| `intern_async_events` | `.events(...)` | `GET .../async/events` |
| `intern_async_tail` | `.tail(...)` | stream / long-poll |
| `intern_async_pause` / `resume` / `intervene` / … | aliases | same send path |

### 3.4 What MCP/SDK must not do

- Require create-N-assignments as the primary UX.  
- Call Factory/Run APIs as a substitute for Intern-owned coordination.  
- Treat receipt or MCP connection as “Intern finished.”  
- Hold Temporal open as liveness.  
- Use a different idempotency namespace than FE.  
- Expose Temporal queries as product state.  
- Conflate with **internal** sandbox agent MCP (in-cycle tools).

### 3.5 Done when

See proof checklist in `intern_async_sdk_mcp_interface.md` §13
(ensure → send → disconnect → events; idempotent retry; pause fence).

---

## 4. Side-by-side

| Concern | Sync ↔ FE | Async ↔ FE | Async ↔ MCP/SDK |
|---|---|---|---|
| Presence | Operator live | Optional | Optional |
| Primary UX | Workbench + chat | Mailbox desk (later) | Queue tools / methods |
| Cardinality | many sync sessions | **one** Async instance | **same** one instance |
| Create | sync-sessions | ensure singleton | `ensure` |
| Writes | commands | messages/commands | **same** send |
| Live delivery | SSE required for good UX | SSE optional | stream/poll optional |
| Research surfaces | Live Experiments/Visuals/Data panes | On-demand from refs | get/list by id |
| Leave tab / disconnect | Disconnect only | Runtime continues | Runtime continues |
| Progress authority | Events + projection | Events + projection + wakes | Same projection |

---

## 5. Frozen v1 command_kinds (cross-adapter)

**Sync:** `operator_message`, `pause`, `resume`, `intervene`,
`answer_interaction`, `close`

**Async (mailbox `kind`):** `message`, `intervene`, `pause`, `resume`,
`redirect_objective`, `answer_interaction`, `request_checkpoint`,
`cancel`, `stop`

Receipt `status` enum shared. Generations are per-runtime
(`sync_session_generation` vs `async_instance_generation` — one Async
generation counter per org Intern).

---

## 6. OpenAPI / codegen expectation

Single source: backend contracts → OpenAPI → FE `smr-openapi.ts` + SDK types +
MCP tool schemas. When this doc and OpenAPI disagree, **fix OpenAPI to match
this boundary** (or amend this doc explicitly)—do not let adapters diverge.

Legacy Magi turn routes remain non-boundary for new work.
