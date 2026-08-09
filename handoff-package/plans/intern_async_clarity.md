# Async Intern — Clarity Spec

**Date:** 2026-07-31  
**Status:** Binding clarity for Async always-on runtime + delegation desk  
**Product role:** Always-on research assistant; no browser required to progress  

Sync = live workbench (see `intern_sync_clarity.md`). Seraph = later.

---

## 1. What Async is

Async is the delegated Intern: it **keeps research moving 24/7** via Factories,
Efforts, Runs/Swarms, optimizers, and related systems within bounds. Operator
may leave; progression must not depend on SSE, MCP, or tab focus.

Frontend/MCP are optional control and observation channels (intervene, pause,
review checkpoints). Dense live Experiments/Visuals/Data panes are a **Sync**
differentiator; Async shows the same resources in timeline/checkpoint/evidence
panels when cited, not a Sync-style multi-pane workbench.

---

## 2. Runtime identity

```text
one Async runtime row per org / research_intern_id
async_runtime_id (canonical; async_assignment_id is a compatibility alias)
InternAsyncWorkflow = intern-async:{org_id}:{research_intern_id}
state_generation
last_event_sequence
cycle_number
next_wake_at
checkpoint (product Intern checkpoint)
budget / evidence_readiness / external_execution_status / blocker
leave_safe = true
```

Sync session cardinality is unrelated: an org may have many Sync sessions, but
creating or attaching one never creates another Async runtime.

Statuses (`AsyncStatus`):  
`created | planning | executing_cycle | checkpointing | sleeping | reconciling | awaiting_input | awaiting_evidence | paused | blocked | cancelling | cancelled | completed | failed`

---

## 3. Engine path

```text
Ensure org Async runtime → singleton PG row + org-stable WF start
→ durable ticks / wakes (observed_at, never now())
→ bounded cycle: propose → legality → policy → effects → product checkpoint
→ sleep / await event / await evidence
→ intervene/pause via commands anytime
```

`Run terminal ≠ evidence ready ≠ Intern complete` → `awaiting_evidence` with
typed blocker/partial on SLA.

---

## 4. Commands (v1)

| command_kind | Purpose |
|---|---|
| `intervene` | Mid-run instruction |
| `pause` / `resume` | Fence new Intern effects (default pause policy) |
| `redirect_objective` | Explicit objective revision |
| `answer_interaction` | Clarification |
| `request_checkpoint` | Force product checkpoint |
| `cancel` / `stop` | Terminal |

---

## 5. Frontend shell

Route: `/smr/intern/async`  
Hero: leave-safe copy, next wake, checkpoint, activity timeline, budgets,
evidence readiness — per `intern_frontend_interaction_spec.md` §8.

---

## 6. E2E proof

**`intern_async_leave_cycle_intervene`** — disconnect during cycle; checkpoint;
intervene once; pause fence.

**`fe_intern_async_leave_safe`** — create, leave, return, same ids, intervene UI.

---

## 7. Done when

- Always-on progress with zero clients.  
- Product checkpoints in Postgres (not Temporal history alone).  
- Pause/intervene/evidence-wait typed and honest.  
- Desk UI is not Sync cockpit with mode=async.
