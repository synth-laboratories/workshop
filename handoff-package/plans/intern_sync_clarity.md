# Sync Intern — Clarity Spec

**Date:** 2026-07-31  
**Status:** Binding clarity for Sync runtime + live frontend workbench  
**Product role:** Live, dynamic research assistant in the signed-in frontend  

Async = always on (separate doc). Seraph = later.

---

## 1. What Sync is

Sync is the operator-present Intern: a **responsive real-time research
assistant** that:

1. Converses with the operator (turns, clarifications, plans).
2. Orchestrates Swarms / Runs when authorized.
3. **Live-visualizes** the research world it touches — **Experiments, Visuals,
   data bindings/datasets**, Runs/actors, costs, and evidence — updating while
   the operator stays on the page.

It is a workbench, not a chatbot with optional links.

---

## 2. Runtime identity

```text
sync_session_id
InternSyncWorkflow = intern-sync:{org_id}:{sync_session_id}
state_generation     (per session; CAS / expected_generation on commands)
last_event_sequence  (cursor)
RuntimeBinding       { factory_id, project_id, effort_id, run_id? }
```

Statuses (OpenAPI `SyncStatus`):  
`created | ready | thinking | waiting_for_operator | paused | closing | closed | failed`  
(+ evidence/finalizing honesty via projection fields or events when Run ends).

---

## 3. Engine path (same shared pattern)

```text
FE command → inbox → Temporal Update → pure Sync reducer
→ PG txn (event + projection + effects + outbox)
→ agent proposes → policy → control activities
→ receipts → SSE events → FE transcript + pane refresh triggers
```

Reducer: no I/O, no clock, no model calls.  
HTTP returns `InternRuntimeCommandReceipt`, not the NL reply.

---

## 4. Commands (v1)

| command_kind | Purpose |
|---|---|
| `operator_message` | Live chat / research instruction |
| `pause` / `resume` | Session control |
| `intervene` | Steer attached Run/Swarm |
| `answer_interaction` | Clarification / approval |
| `close` | Typed terminal / research summary |

Create: `POST /smr/research-intern/sync-sessions`.

---

## 5. Live research surfaces (required)

Scoped by session `RuntimeBinding.project_id` / `run_id`:

| Surface | Why Sync must show it live |
|---|---|
| **Experiments** | Sync discusses and steers experimental work; operator must see bundles/compare/status update in-session |
| **Visuals** | Results and Visual publications are first-class research outputs; appear as Runs/experiments complete |
| **Data** | Bindings + dataset revisions the Intern uses/creates must be inspectable, not only described in chat |
| **Run / actors** | Orchestration target readiness and lifecycle |
| **Costs / usage** | Live spend against bounds |
| **Evidence / artifacts** | Attributable links; archive-finalizing honesty (A7) |

### Refresh triggers

1. Intern events with `resource_refs` (primary).  
2. Window focus.  
3. Light poll only while session is `thinking` (not sole authority).  
4. Manual refresh.

### Non-goals for panes

- Panes are not a second Intern state machine.  
- No writing Experiments/Visuals/Data except via Intern commands or navigating
  to owner pages that use their own APIs.  
- No fabricating Visuals from chat text.

---

## 6. Frontend shell

Route: `/smr/intern/sync/[syncSessionId]`  
Contract details: `intern_frontend_interaction_spec.md` §7.

Modules:

```text
sync/SyncCockpit.tsx
sync/SyncTranscript.tsx
sync/SyncComposer.tsx
sync/panes/
  ExperimentsPane.tsx
  VisualsPane.tsx
  DataPane.tsx
  RunActorsPane.tsx
  CostsPane.tsx
  EvidencePane.tsx
shared/useRuntimeEventStream.ts  # also fans out resource_refs → pane invalidation
```

---

## 7. Event → pane coupling

Intern events SHOULD include:

```text
resource_refs: [{ kind, id, project_id?, run_id?, ... }]
```

Kinds: `experiment | visual | data_binding | dataset_revision | run | artifact | candidate`

FE: on event, invalidate the matching pane query and highlight the resource.

---

## 8. E2E proof (Sync)

**`intern_sync_live_turn_reconnect`** (backend) — turns + cursor reload.  

**`fe_intern_sync_live_surfaces`** (Playwright, required for Sync clarity):

1. Sign in → create Sync session on bound project/run.  
2. Send message that causes or references research activity.  
3. Assert Experiments and/or Visuals and/or Data pane shows a real server id
   (not placeholder empty forever when resources exist).  
4. Reload → transcript + pane selections restore from projection/events.  
5. When Run goes terminal with evidence still finalizing, UI shows honest
   finalizing state — not “complete”.

---

## 9. Done when

- Sync reducer + `InternSyncWorkflow` + command receipts.  
- FE workbench: transcript **and** live Experiments + Visuals + Data panes.  
- resource_refs from events drive pane focus.  
- Reload/SSE rock solid per FE interaction spec.  
- No Magi five-receipt chain as Sync product definition.
