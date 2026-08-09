# Intern Frontend Interaction Spec (Rewrite Contract)

**Date:** 2026-07-31  
**Status:** Binding for full frontend rewrite — makes FE↔backend interaction rock solid  
**Product:** Async always-on · Sync live/dynamic · Seraph later  

Implements against goal design + OpenAPI shapes already present:

- `SyncSession*` / `AsyncAssignment*`
- `InternRuntimeCommandRequest` / `InternRuntimeCommandReceipt`
- Runtime event stream + cursor

Legacy `/smr/research-intern/sessions/.../turns` and Magi receipt-chain UI are
**deprecated** for the rewrite. Do not extend `client.tsx` cockpit; replace.

---

## 1. Principles (non-negotiable)

1. Frontend is an **adapter**, never a transition authority.
2. Browser stores only: `runtime_kind`, `runtime_id`, last durable **cursor**,
   in-flight `command_id`s for optimistic reconcile — never reconstructed state.
3. **Writes = commands.** **Reads = projections + events.** **Live = SSE.**
4. Operator chat text is a command payload; Intern replies are **events**.
5. SSE/heartbeats never imply progress; only ledger events do.
6. Closing the tab never stops Async; Sync merely disconnects observers.
7. Sync shell ≠ Async shell. Separate routes/components; no shared “mode” on one session.

---

## 2. Signed-in auth

### 2.1 Browser session

- User signs in via existing Clerk (or org) protected app shell.
- Intern page lives under protected SMR routes: `/smr/intern/...`.
- All Intern API calls use the **same authenticated SMR client** already used
  for Factory/Project (session cookie + BFF, or bearer from Clerk → backend
  exchange — **whatever the rest of `/smr/*` uses today**). Do not invent a
  second Intern-only auth path.

### 2.2 Backend binding

On each request the API resolves:

```text
authenticated user → org_id → Research Intern (get-or-create)
→ authorize operator on that Intern
```

Frontend never sends `org_id` as authority. It may display `research_intern_id`
from GET `/smr/research-intern`.

### 2.3 Failure UX

| HTTP | FE behavior |
|---|---|
| 401 | Re-auth / sign-in |
| 403 | Typed denied (capability / membership) |
| 409 conflict | Refresh projection; rebase optimistic commands |
| 422 | Validation; show `decision_code` / field errors |
| 503 retryable | Retry with backoff; keep cursor |

---

## 3. Resource model (URL + IDs)

```text
/smr/intern                          → shell chooser (Sync | Async)
/smr/intern/sync                     → Sync list + create
/smr/intern/sync/[syncSessionId]     → Sync cockpit
/smr/intern/async                    → Async list + create
/smr/intern/async/[assignmentId]     → Async desk
```

Backend resources (canonical):

```text
GET/POST  /smr/research-intern
GET/POST  /smr/research-intern/sync-sessions
GET       /smr/research-intern/sync-sessions/{sync_session_id}
POST      /smr/research-intern/sync-sessions/{sync_session_id}/commands

GET/POST  /smr/research-intern/async-assignments
GET       /smr/research-intern/async-assignments/{assignment_id}
POST      /smr/research-intern/async-assignments/{assignment_id}/commands

GET       /smr/research-intern/runtimes/{runtime_kind}/{runtime_id}/events
GET       /smr/research-intern/runtimes/{runtime_kind}/{runtime_id}/events/stream
GET       /smr/research-intern/runtimes/{runtime_kind}/{runtime_id}/projection
```

Until unified runtime event routes ship, FE may temporarily use
session-shaped event streams **only if** they are keyed by
`sync_session_id` / `async_assignment_id` and return the same cursor contract.
Do not keep the old Magi turn waiter as the write path.

**IDs the UI must show (copyable):** `research_intern_id`, runtime id,
`state_generation`, `last_event_sequence` / cursor, `command_id`, checkpoint id
(Async), `temporal_workflow_id` (debug drawer).

---

## 4. Command path (all writes)

### 4.1 Request

```json
{
  "command_id": "cmd_<ulid>",
  "idempotency_key": "fe:<ulid>",
  "expected_generation": 12,
  "command_kind": "operator_message",
  "payload": { }
}
```

FE generates `command_id` + `idempotency_key` client-side **before** send.
Retries reuse the same pair. `expected_generation` = last known projection
`state_generation`.

### 4.2 Response — `InternRuntimeCommandReceipt`

```text
status: received | delivered | applied | noop | refused | superseded | conflict
```

| Status | UI |
|---|---|
| `received` / `delivered` | “Accepted for processing…” — wait on events |
| `applied` / `noop` | Clear optimistic pending for that command |
| `refused` / `superseded` | Show `decision_code`; do not invent success |
| `conflict` | Refetch projection; update generation; offer retry |

**HTTP returns command admission/decision — not the Intern’s natural-language
reply.** Reply arrives as events on the stream.

### 4.3 Sync command_kinds (v1)

| kind | payload (min) | Purpose |
|---|---|---|
| `operator_message` | `{ "body": "..." }` | Live chat turn |
| `pause` | `{ "rationale": "..." }` | Pause session |
| `resume` | `{ "rationale": "..." }` | Resume |
| `intervene` | `{ "body", "state_patch"? }` | Steer attached work |
| `close` | `{ "status", "rationale" }` | Terminal close |
| `answer_interaction` | `{ "interaction_id", "answer" }` | Clarification |

### 4.4 Async command_kinds (v1)

| kind | payload (min) | Purpose |
|---|---|---|
| `intervene` | `{ "body", "state_patch"? }` | Mid-run instruction |
| `pause` / `resume` | rationale | Fence / unfence |
| `redirect_objective` | `{ "objective" }` | Explicit revision |
| `answer_interaction` | interaction bind | Clarification |
| `request_checkpoint` | optional note | Force product checkpoint |
| `cancel` / `stop` | rationale | Terminal |

Create uses POST create resources (not generic commands):

- Sync: `POST .../sync-sessions` → `SyncSessionResponse`
- Async: `POST .../async-assignments` → `202` semantics: show `leave_safe`,
  status may be `provisioning` until workflow ready — **never** claim running
  until projection says so.

---

## 5. Read path

### 5.1 Projection (product truth)

On enter / focus / conflict / reconnect:

```text
GET projection for runtime
→ replace local view model from server
→ set expected_generation = state_generation
→ set cursor = last_event_sequence (+ event_id if provided)
```

Sync projection fields (min): status, binding, pending_turn_id, generation,
cursor, temporal_workflow_id.  
Async projection fields (min): status, cycle_number, plan, next_wake_at,
checkpoint, budget, evidence_readiness, external_execution_status, blocker,
leave_safe.

### 5.2 Event history

```text
GET .../events?after_sequence=N&limit=...
```

Render timeline from events. Dedupe by `event_id`. Order by
`runtime_sequence` / `sequence`.

### 5.3 Linked resources (not Intern state)

Separate authenticated GETs — display as panels/links, never merge into Intern
reducer:

- Factory / Project / Effort / Run status  
- Experiments, Visuals, datasets  
- Usage / cost  
- Artifacts / Wasabi manifests (by artifact_id from events)

Mutations to those systems go through Intern **commands** (or their own pages),
not silent FE writes.

---

## 6. Live path (SSE)

### 6.1 Connect

```text
GET .../events/stream?after_sequence=N[&after_event_id=...]
Authorization: same as other SMR calls
```

Frames:

```text
event: intern_event
data: { kind: "event", runtime_id, cursor, event }

event: intern_heartbeat
data: { kind: "heartbeat", runtime_id, cursor?, reconnect_after_ms, emitted_at }
```

Heartbeats: refresh liveness UI only. **Do not** advance research timeline.

### 6.2 Reconnect algorithm (rock solid)

```text
1. On load: GET projection
2. GET events after stored cursor (backfill gap)
3. Open SSE after latest cursor
4. On event: if event_id seen → ignore; else append; advance cursor
5. On SSE drop: exponential backoff; goto 1 (projection first)
6. If Redis trimmed / 410-style gap: full PG backfill then SSE
```

Persist cursor in `sessionStorage` keyed by
`intern:{runtime_kind}:{runtime_id}:cursor`.

### 6.3 Optimistic UI

- On send: show local bubble tagged `command_id`, state `sending`.  
- On receipt `applied|delivered|received`: mark `accepted`.  
- On Intern reply event citing command / correlation: mark `settled`.  
- On `conflict|refused`: mark failed; refetch projection.  
- Never invent Intern text locally.

---

## 7. Sync cockpit UX (live / dynamic research workbench)

Sync is not chat-only. It is a **live research workbench**: converse with the
Intern **and** continuously visualize the research surfaces it orchestrates —
Experiments, Visuals, datasets/data bindings, Runs/actors, costs, and evidence —
updating while the operator is present.

### 7.1 Layout

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ Sync · live     status  gen  cursor  pending_turn?                       │
│ Factory / Project / Effort / Run   [runtime-ready]                       │
├────────────────────────────┬─────────────────────────────────────────────┤
│ Transcript (Intern events) │ Live research panes (tabbed / stacked)      │
│  · operator_message        │  ● Experiments   (list, compare, bundle)  │
│  · agent_message + proven. │  ● Visuals       (run/project visuals)    │
│  · progress / blockers     │  ● Data          (bindings, revisions)    │
│  · resource-link events    │  ● Run / actors  (lifecycle, readiness)   │
│                            │  ● Costs / usage                          │
│                            │  ● Evidence / artifacts                   │
│                            │  ● Interactions (clarify / approve)       │
├────────────────────────────┴─────────────────────────────────────────────┤
│ Composer → operator_message                                              │
│ [Pause] [Intervene] [Close]                                              │
└──────────────────────────────────────────────────────────────────────────┘
```

Minimum viewport: transcript + **at least one** live pane visible. Desktop:
transcript left (~45%), live panes right. Mobile: transcript primary; panes in
bottom sheet / tabs — Experiments / Visuals / Data must remain one tap away,
not buried in a generic “links” list.

### 7.2 Live research panes (first-class)

Each pane is a **read projection** from existing SMR owner APIs scoped by the
Sync session `RuntimeBinding` (`factory_id`, `project_id`, `effort_id`,
`run_id`). Panes refresh on:

1. Sync Intern **events** that cite resource ids (preferred trigger),
2. Focus / visibility regain,
3. Short poll while `status ∈ {thinking, awaiting_action}` (≤5–10s), never as
   the sole progress authority,
4. Explicit Refresh.

| Pane | Reads (examples) | Shows live |
|---|---|---|
| **Experiments** | `GET .../projects/{project_id}/experiments...`, bundles, compare | Active/recent experiments, status, score/delta when present, open bundle, compare selected |
| **Visuals** | `GET /smr/visuals`, run/project visual reads | Published Visuals for bound project/run; thumbnail/title; open `/v` or owner URL |
| **Data** | `GET .../data-bindings`, revisions, datasets | Bindings, revision lifecycle, freshness; open revision detail |
| **Run / actors** | run status, actors, runtime-ready | Public state, orchestrator readiness, blockers |
| **Costs** | run/project usage | Spend vs session bounds if exposed |
| **Evidence** | evidence refs from Intern events + artifact manifests | Linked claims, archive readiness honesty |

**Authority rule:** panes never invent Intern state. They show owner-system
truth. Intern transcript explains/actuates; panes verify what exists.

### 7.3 How Intern drives the panes

Intern agent replies and progress events should carry **structured resource
refs** when they touch research objects, e.g.:

```text
payload.resource_refs: [
  { kind: "experiment", id, project_id },
  { kind: "visual", id, run_id? },
  { kind: "data_binding", id, project_id },
  { kind: "dataset_revision", id, data_binding_id },
  { kind: "run", id, project_id }
]
```

FE behavior:

- Highlight / auto-select the cited pane and resource.
- Deep-link “Open experiment / visual / data” without leaving Sync when
  possible (side panel); full page route as secondary.
- If Intern proposes a mutation (new experiment, publish visual, revise data),
  that is still a **command → policy → receipt** path — pane only updates after
  owner projection changes / events arrive.

### 7.4 Sync-only product expectation

Operators must be able to:

- Ask Sync about an experiment/visual/dataset and **see that object** update in
  the pane during the same session.
- Watch Visuals and experiment bundles appear as Runs complete (with
  evidence-finalizing honesty when archive/traces lag).
- Inspect data bindings/revisions the Intern is using — not only chat summaries.

Async may link the same resources in timeline/checkpoint; it does **not** need
this dense live multi-pane workbench. That density is a Sync differentiator.

### 7.5 Create / wait / honesty

**Create gate:** if binding requires live Run, soft `runtime-ready` probe;
disable Start until ready (or show typed blockers).

**Waiting:** status `thinking` / pending_turn_id → Recover only if command stuck
past SLA; recovery = refetch projection + resume SSE.

**Evidence honesty:** Run execution terminal + evidence not ready → banner
“Run completed; archive/evidence still finalizing” — panes may show Visuals /
experiments as partial until `evidence_readiness` / archive finalized.

---


## 8. Async desk UX (always on)

**Hero:** delegation + supervision, not chat spinner.

```text
┌─────────────────────────────────────────────────────────┐
│ Async · always on   status  next_wake  budgets  leave_safe │
│ Latest checkpoint id · cycle N · evidence_readiness       │
├───────────────┬──────────────────┬────────────────────────┤
│ Activity      │ Checkpoint       │ Evidence / blocker     │
│ timeline      │ panel            │ panel                  │
│ (events)      │ (product CP)     │                        │
├───────────────┴──────────────────┴────────────────────────┤
│ Intervene / Pause / Resume / Redirect / Answer / Cancel   │
│ Copy: “You may leave; closing this tab does not stop it.” │
└─────────────────────────────────────────────────────────┘
```

**Create:** POST assignment → show durable id + leave-safe copy immediately;
if `provisioning`, poll/SSE until active — do not claim cycle work yet.

**Tab backgrounded:** keep SSE optional; on focus run reconnect algorithm.
Async progress continues server-side regardless.

---

## 9. Interactions (clarify / approve)

When event `InteractionRequested` arrives:

```text
{ interaction_id, question, response_schema, deadline, blocking, ... }
```

UI: modal or rail card. Answer via `answer_interaction` command bound to
`interaction_id`. First valid wins; later → `superseded` toast.

---

## 10. FE module structure (rewrite)

Delete/stop extending legacy Magi chain cockpit as the product. Target:

```text
src/app/(pages)/(protected)/smr/intern/
  page.tsx                 # shell switcher
  sync/
    page.tsx               # list + create
    [syncSessionId]/page.tsx
    SyncCockpit.tsx        # workbench: transcript + live panes
    SyncComposer.tsx
    panes/
      ExperimentsPane.tsx  # REQUIRED live
      VisualsPane.tsx      # REQUIRED live
      DataPane.tsx         # REQUIRED live (bindings/revisions)
      RunActorsPane.tsx
      CostsPane.tsx
      EvidencePane.tsx
  async/
    page.tsx
    [assignmentId]/page.tsx
    AsyncDelegationDesk.tsx
    CheckpointPanel.tsx
    ActivityTimeline.tsx
  shared/
    InternProvider.tsx
    useRuntimeProjection.ts
    useRuntimeEventStream.ts   # also invalidates panes on resource_refs
    useRuntimeCommand.ts
    useBoundResearchReads.ts    # experiments/visuals/data GETs by binding
    ConflictBanner.tsx
    TypedFailure.tsx

src/lib/intern/
  commands.ts
  cursor.ts
  receipts.ts
  resourceRefs.ts
  api.ts
```

`useRuntimeCommand`:

```text
assert expected_generation from projection
POST command
handle receipt statuses
on conflict → invalidate projection query
```

`useRuntimeEventStream`: reconnect algorithm §6.2 only.

---

## 11. State the FE is forbidden to keep as truth

- Reconstructed “current plan” without projection/checkpoint  
- Magi five-step stage machine as Async stage  
- Heartbeat-based “still working” as progress rows  
- Local completion when Run public_state terminal  
- Second client-generated Intern reply  

---

## 12. Parity with MCP / SDK

Same commands, same receipt statuses, same event cursor, same projection
fields. FE may be prettier; it must not have a private write API.

---

## 13. Acceptance tests (frontend rewrite)

### Sync Playwright — `fe_intern_sync_live_reconnect`

1. Sign in → open Sync create → bind ready Run → create session.  
2. Send message → see command receipt → see agent event with ids.  
3. Reload page → same `sync_session_id`, transcript restored, cursor advanced.  
4. Send second message → second reply; no dup first reply.  
5. Conflict simulation (stale generation) → banner + recovery.

### Async Playwright — `fe_intern_async_leave_safe`

1. Sign in → create Async assignment → see leave-safe copy + id.  
2. Close tab / navigate away ≥ cycle SLA (or inject checkpoint via test hook
   against real backend).  
3. Return → same `async_assignment_id`, checkpoint visible, budgets from server.  
4. Intervene → receipt → timeline shows intervention event.  
5. Pause → UI shows paused; no “working” spinner implying new cycles.

Backend E2Es in `intern_runtime_scope.md` remain authoritative for runtime;
these prove FE adapter correctness.

---

## 14. Migration

1. Land this spec + keep OpenAPI `InternRuntimeCommand*` / sync-async resources.  
2. Implement `src/lib/intern/*` + hooks against new routes.  
3. Ship Sync pages; feature-flag old `client.tsx`.  
4. Ship Async desk; remove Magi-chain-as-product UI.  
5. Delete legacy turn/cockpit paths from FE when backend deprecates.  
6. Only then call FE interaction rock solid.

---

## 15. Definition of rock-solid FE interaction

- Signed-in user uses one SMR auth path.  
- Every write is a versioned, idempotent command with typed receipt.  
- Every live update is a deduped ledger event after a durable cursor.  
- Reload never loses Sync transcript or Async assignment identity.  
- Async leave-safe is true in UX and runtime.  
- Conflicts and evidence-finalizing states are typed, never infinite spinners.  
- Sync and Async are separate shells bound to separate runtime IDs.
