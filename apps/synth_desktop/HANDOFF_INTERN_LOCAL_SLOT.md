# Handoff: Local Slot Backend + Sync / Async Intern (Desktop)

**Date:** 2026-08-08  
**Audience:** Engineer improving Cloud Intern in `apps/synth_desktop` + `services/local-runtime`  
**Status:** v0 has demo + thin remote mailbox; this doc is the path to honest local-slot + Sync workbench + Async leave-safe  
**Related:** `apps/synth_desktop/HANDOFF.md`, `handoff-package/plans/intern_sync_clarity.md`, `intern_async_clarity.md`, `excerpts/backend/local_pilot.py`, `references/understudy_README.md`

---

## 0. One-liner

> Desktop stays a **viewer**. The daemon adapts **one Intern mailbox** (sync sessions + org async singleton) and, separately, a **local slot** as an execution host the cloud Intern can lease — never a second commander-queue, never Intern HTTP from the renderer.

Today’s Cloud UI (Sync list + pinned Async + Inventory containers labeled “slot”) is the right IA. Under the hood, “local slot” is still a **demo label**, Sync is **messages + activity**, and Async is **demo/remote poll** with a hard-coded leave-safe banner.

---

## 1. What we have today (honest)

```text
Electron Cloud desk
  └── runtimeClient → /v1/sessions (target: intern sync|async)
        └── InternAdapter
              ├── demo  (SYNTH_INTERN_DEMO=1): fake ids, scripted events
              └── remote (SYNTH_API_KEY + SYNTH_INTERN_DEMO=0):
                    POST/GET /smr/research-intern/sync-* and /async/*
                    HTTP poll (~0.35–0.9s) → RuntimeEvent → SSE to UI
```

| Surface | Works | Doesn’t |
|---------|--------|---------|
| Sidebar **Cloud → Sync sessions** | Create, open desk, send, pause/resume/close | Presence, intervene, answer_interaction, research panes |
| Sidebar **Async Intern** pin | Singleton session, pause/resume/checkpoint/cancel | Provide input (toast stub), real phase/wake/budget UI |
| Leave-safe host | Electron quit ≠ kill daemon | Banner always-on for async; not projection-driven |
| Inventory “slot” | Demo container `craftax-pool-slot` + `metadata.slot` | No lease, Understudy pin, Local Pilot, or pool provision |
| Transcript | User `message.*` | Intern `agent_message` stays in **activity**, not chat bubbles |
| Bindings | Null factory/project/effort on create/ensure | No project → `RuntimeBinding` |

**Modes (`RuntimeConfig.intern_mode`):**

| Mode | When | Behavior |
|------|------|----------|
| `remote` | `SYNTH_API_KEY` set and demo not forced | Real mailbox HTTP |
| `demo` | `SYNTH_INTERN_DEMO=1` | Scripted local events |
| `unconfigured` | No key, demo off | Blocks send with configure message |

Docs sometimes imply “no key ⇒ demo”; code defaults demo **off**. Align boot UX with that.

---

## 2. Target architecture (three cooperating pieces)

Keep these distinct — conflating them is how we get a fake second Intern:

```text
┌─────────────────────────────────────────────────────────────────┐
│  A. INTERN MAILBOX (cloud authority)                            │
│     Sync sessions (N) + Async org singleton (1)                   │
│     Commands + events + generation fences                         │
│     Desktop mirrors via daemon InternAdapter                      │
└────────────────────────────┬────────────────────────────────────┘
                             │ may bind / lease
┌────────────────────────────▼────────────────────────────────────┐
│  B. LOCAL SLOT (execution host on this machine)                   │
│     Understudy-pinned guest / synth-container / Local Pilot lease │
│     Inventory: Containers row with location=local + slot id       │
│     Not a mailbox. Not a sync session.                            │
└────────────────────────────┬────────────────────────────────────┘
                             │ optional turn execution
┌────────────────────────────▼────────────────────────────────────┐
│  C. LOCAL PILOT (dev-only Sync turn host)                         │
│     Backend excerpt: leases + capabilities for Sync Local Pilot   │
│     Daemon presents loopback lease status; product ops still via  │
│     Intern command/effect/MCP pipeline                            │
└─────────────────────────────────────────────────────────────────┘
```

**Product IA stays:** Chats/ (local Laguna + ACP remotes) · Cloud/ (Sync list + Async pin) · Inventory (Containers / Traces / Visuals).

---

## 3. Better Sync Intern support

### 3.1 Product bar (from clarity spec)

Sync = operator-present research **workbench**: converse + orchestrate + **live-visualize** Experiments / Visuals / Data / Runs / Costs while the operator stays on the page.

### 3.2 Desktop work

1. **Transcript honesty** — Map mailbox NL / `agent_message` (and demo equivalents) into `eventsToMessages`, not only activity. Align demo event kinds with remote (`resource_ref.created` not ad-hoc `resource.linked`).
2. **Generation-fenced controls** — Expose intervene / answer_interaction / approve paths through daemon `/v1/sessions/{id}/commands` (don’t invent renderer Intern HTTP). Reuse FE patterns from handoff excerpts (`researchIntern.ts`, Sync presence).
3. **Presence** — Surface thinking / waiting_for_operator / paused from projection; don’t invent a second status enum.
4. **RuntimeBinding** — Selected Workshop project → `{ factory_id, project_id, effort_id, run_id? }` on `create_sync` / commands. Empty nulls are why Sync feels unbound.
5. **Workbench panes (incremental)** — Driven by `resource_refs` + Inventory openers (Trace / Visual), not a second state machine. Order: Visuals cite → open pane; then Runs/costs stubs.
6. **Activity filter** — Keep All / Mailbox; Mailbox = commander-queue authority only (already directionally correct).

### 3.3 Key files

- `services/local-runtime/.../adapters/intern.py`, `intern_client.py`
- `apps/synth_desktop/.../CloudDesk.tsx`, `runtime/sessionView.ts`, `App.tsx`
- Clarity: `handoff-package/plans/intern_sync_clarity.md`

---

## 4. Better Async Intern support

### 4.1 Product bar

Async = always-on org singleton; progress **must not** depend on Desktop being open. UI is control + observation (intervene, pause, checkpoints, evidence), not a Sync multi-pane workbench.

### 4.2 Desktop work

1. **Projection-driven pin** — Sidebar pin + desk header show real fields from async projection: `phase` (noun:verb if available), `cycle_number`, `next_wake_at`, `checkpoint`, `budget`, `blocker`, `evidence_readiness`, `leave_safe`, `needs_input`.
2. **Leave-safe banner** — Show only when projection says leave-safe (or remote mode); today it’s hard-coded for all async desks. Keep daemon-alive-on-quit.
3. **Provide input / intervene / redirect_objective** — Replace toast stub; route through daemon → `async/messages` or `async/commands` with generation fence.
4. **Checkpoint UX** — `request_checkpoint` already exists; show last checkpoint + open cited Trace/Visual from Inventory.
5. **Cardinality** — Enforce one async session in UI (already singleton in `create_session`); never list Async as parallel “sessions” next to Sync.

### 4.3 Key files

- Same Intern adapter + `service.py` async singleton
- `Sidebar.tsx` async pin, `CloudDesk.tsx` async branch, `sessionView.mapAsyncPhase`
- Clarity: `handoff-package/plans/intern_async_clarity.md`, async phase handoffs under `handoff-package/references/`

---

## 5. Better local slot backend support

### 5.1 What “local slot” means here

Not “another Intern.” A **leased execution environment on this Mac** that cloud Sync/Async (or local Laguna) can use:

| Layer | Role |
|-------|------|
| **Inventory · Containers** | Discover / register local containers + (later) org pool slots |
| **Understudy pin** | Version-pinned guest beside the slot (`handoff-package/references/understudy_README.md`) |
| **Local Pilot** | Dev-only Sync turn host: short-lived lease + capabilities; fail-closed (`local_pilot.py` excerpt) |
| **Daemon** | Owns lease status, health probe, pointers in SQLite; never stores cloud secrets in renderer |

### 5.2 Implementation sequence for slots

1. **Promote inventory from demo labels** — Real `location: local | cloud`, health probe to `baseUrl`, optional `slotId` / `poolId` / understudy version in metadata.
2. **Daemon SlotBroker (thin)** — `GET/POST /v1/slots` (or extend `/v1/containers`): register local endpoint, heartbeat, last reward / last rollout pointer. Still not a mailbox.
3. **Wire RuntimeBinding + slot pointer** — When Sync/Async is bound to a project that uses local execution, pass slot/container id in metadata; Intern cloud remains authority for commands.
4. **Local Pilot loopback (opt-in)** — Env `INTERN_LOCAL_PILOT_ENABLED=1` + loopback-only: daemon obtains lease from synth-dev, executes allowed Sync turn tools, returns effects via Intern pipeline. Desktop shows lease TTL / grant status in Cloud desk or Inventory.
5. **Pool attach** — Only after local lease works: list remote pool slots user can tunnel; click → Inventory + “use for next Sync run.”

Do **not** start by cloning Sync sessions into “local Intern sessions.”

### 5.3 Key files

- `services/local-runtime/.../inventory.py` (seed `craftax-pool-slot` is fixture only)
- `handoff-package/excerpts/backend/local_pilot.py`
- `handoff-package/references/understudy_README.md`
- `handoff-package/docs/02-REUSE-GAP-MATRIX.md` (Local Pilot / reuse)

---

## 6. Recommended build order

1. **Transcript + event-kind parity** (demo ↔ remote) so Cloud desk feels like a conversation.  
2. **Boot mode UX** — Configure `SYNTH_API_KEY` / explicit demo; health pill already shows Intern mode.  
3. **Async projection UI** — phase, wake, checkpoint, needsInput, Provide input.  
4. **Sync controls** — answer_interaction / intervene + presence.  
5. **RuntimeBinding from selected project.**  
6. **Inventory containers → real local health**; drop “slot” as a string-only seed.  
7. **Local Pilot opt-in loopback** against a synth-dev slot.  
8. **Sync research panes** via resource_refs → Inventory / VisualPane (clarity §5).

---

## 7. What NOT to do

- Call `/smr/research-intern/*` from the Electron renderer  
- Invent a second mailbox / commander-queue for “local Intern”  
- Treat Inventory demo `metadata.slot: "local-slot"` as a real lease  
- Kill the daemon on window close (breaks Async leave-safe)  
- Fork Next.js SyncCockpit wholesale — extract patterns, keep DesktopRuntime boundary  
- Put Local Pilot credentials in renderer storage  

---

## 8. Dogfood checklist (after each slice)

```bash
# Demo path
SYNTH_INTERN_DEMO=1 npm run dev:desktop
# → Sync send shows Intern reply in transcript; Async pin shows phase; Provide input works

# Remote path
SYNTH_API_KEY=... SYNTH_INTERN_DEMO=0 npm run dev:desktop
# → create Sync against bound project; events with remote_sequence; leave-safe daemon survives quit

# Slot path (later)
# → Inventory local container health green; Local Pilot lease visible; Sync turn can use lease
```

---

## 9. File map

| Area | Path |
|------|------|
| Intern adapter | `services/local-runtime/src/synth_local_runtime/adapters/intern.py` |
| Mailbox HTTP | `.../intern_client.py` |
| Config / modes | `.../config.py` |
| Async singleton | `.../service.py` |
| Inventory / seed | `.../inventory.py` |
| Cloud desk | `apps/synth_desktop/src/renderer/src/components/CloudDesk.tsx` |
| Sidebar Cloud | `.../Sidebar.tsx` |
| Event → UI | `.../runtime/sessionView.ts` |
| Protocol | `packages/runtime-protocol` |
| Sync clarity | `handoff-package/plans/intern_sync_clarity.md` |
| Async clarity | `handoff-package/plans/intern_async_clarity.md` |
| Local Pilot | `handoff-package/excerpts/backend/local_pilot.py` |
| Understudy / slot pin | `handoff-package/references/understudy_README.md` |
