# Handoff: Async runtime phase model — ambient backpressure, `noun:verb` status, typed resume

**Date:** 2026-08-08  
**Ship:** intern-async 24/7 (liveness / O4 stall-scan / runtime projection)  
**Status:** Steps 0–7 + deferred follow-ons landed locally (2026-08-08).
Includes finish-pass P0/P1 fixes, L1 WaitOn, L3 model blockers, empty-parked
`BlockAsync` (env-gated OFF by default), `watchdog_at` at stop sites, capability
arg-scoped availability, and wire enums (`RuntimePhaseWire` / `StopReason` /
`ResumeKind`). Not pushed; Josh decides when it goes out.  
**Still optional / ops:** turn on `INTERN_ASYNC_EMPTY_PARKED_BLOCK_ENABLED` after
alert volume is known; regenerate `smr_openapi.yaml` + frontend types if not
already in the same train; capability broader `requires_project` audit.  
**Observed trigger:** acceptance cell `A-Craftax`, run `20260808T144218Z`, slot6  
**Subsystems:** `packages/intern/async_/{state,reducer,phase}.py`, `packages/intern/contracts.py`, `packages/intern/prompts.py`, `services/intern/{runtime_authority,async_workflow_sweeper,product,async_backpressure}.py`  
**Style bar:** `backend/tigerstyle.md` (Synth Style) · `notes/specifications/tanha/references/synthstyle.md`  
**Sibling docs:**
- `INTERN_ASYNC_LIVENESS_HANDOFF_2026-08-08.md` — why nothing noticed the deadlock (F-1/F-2/F-3). **This doc is the structural fix.**
- `INTERN_CAPABILITY_BINDING_HANDOFF_2026-08-08.md` — why the Intern got stuck on cycle 1 (capability). Independent; either can land first.

**Constraints:** Do not push. Land locally; Josh decides when it goes out. Do not synthesize a fake wake time to paper over a deadlock. Alert before auto-`BlockAsync` until false-positive rate is known.

---

## 0. What shipped (implementer summary)

| Layer | Landed |
|---|---|
| Domain | `packages/intern/async_/phase.py` — `noun:verb` phase, typed `ResumeCondition` / `AgentBackpressure`, named waits, invariants, `derive_leave_safe`, `promote_wakeless_empty_parked` |
| Reducer | `_applied` pair-asserts; bare `waiting_for_event` → `async_checkpoint_gated_without_resume` (`BLOCKED` + `outcome=BLOCKED`); AC1 judgment stays wake-less with `HUMAN_ANSWER`; `_normalize_backpressure` repairs COMPAT wedge |
| Projection | persist/rehydrate via `services/intern/async_backpressure.py` (COMPAT promote to gated); `async_response` projects `phase` / `resume` / `leave_safe: bool` / `active_effort_id` / `outcome` |
| Sweeper | empty-parked → `intern_async_sleeping_without_wake` (alert-only); **also pages** `blocker_code=async_checkpoint_without_resume`; judgment never auto-`BlockAsync`; watchdog scan bounded (`LIMIT 50`) |
| Prompt | Async cycle checkpoint example requires RFC3339 `next_wake_at`; null wake only legal with judgment park |
| Smoke | only `async_checkpoint_scheduled` advances `CHECKPOINT_OBSERVED` |
| SDK | synth-ai `InternAsyncRuntime` + vendored OpenAPI; lockstep green |
| Verification | backend units: 123 passed (`phase` + runtime core + leave_safe); synth-ai lockstep + testing SDK fixtures green at land |

### Finish-pass fixes (review → land, same day)

1. **COMPAT wedge:** legacy `SLEEPING` + no wake + gated-shaped derive left `agent:waiting` without a producer → every subsequent `reduce()` asserted. Rehydrate and normalize now **promote** via `promote_wakeless_empty_parked` → `BLOCKED` + stable blocker code so handoff/pause stay commandable.
2. **Gated rows were invisible to alerts:** empty-parked scan skipped all `blocked`. It now pages `async_checkpoint_without_resume` specifically (alert-only).
3. **Prompt taught `next_wake_at: null`:** updated; null without judgment gates the org.
4. **`outcome` omitted** on gated checkpoint path — now `RuntimeOutcome.BLOCKED` like every other block.
5. **Watchdog scan** unbounded full-table — now `LIMIT 50` + docstring that `watchdog_at` population is still follow-up.
6. **Dead `AsyncStatus` members** marked DEPRECATED in the enum docstring.

---

## 1. One-sentence problem

The Async runtime is an **ambient backpressure machine** that admits, defers, gates, or holds an agent — but its status vocabulary pretends the *runtime* “plans / executes / sleeps,” collapses many distinct waits into `sleeping`, and allows an agent to stop with **no named way to start again**.

---

## 2. Product model (what we are aiming at)

### 2.1 Ambient runtime, agent drives

Desired operator mental model:

```
  long-haul agent session
       │
       ├── work… (many tool calls) …compact… work…
       ├── kick swarm ──► wait until run_done OR timeout
       ├── blocked? ──► Block tool ──► Sync / approval gate
       │                    │
       │                    └── resolve or timeout ──► runtime resumes agent
       └── Intern owns the plan; runtime only keeps him going
```

Cycles are **commit / budget / single-effect boundaries**, not an agenda the agent must follow. Inside `agent:running`, the Intern may `file_list` / `file_read` / `project_create` / … many times. After each MCP observation the reducer re-dispatches `RunAsyncCycle` with the **same** `cycle_number` (`reducer.py` ~1330–1344). There is no “at the beginning of a cycle he must do X.”

### 2.2 Sync vs Async (cardinality)

| | Sync | Async |
|---|---|---|
| Presence | Human present | Unattended / always-on |
| Unit | Turn | Tick (`cycle_number`) — platform boundary, not a script |
| Runtime | N sessions / Intern | 0‥1 runtime / org |
| Stop | Session ends | Wait / gate / hold + ambient wake |

Async may open Sync for human handoff; Sync is not “a short cycle.”

### 2.3 Where a cycle sits in `A-Craftax`

Harness: mint org → provision → `async_.ensure` (≤24 cycles / cost cap) → stage Files → **one** `async_.send`.  
Product: many ticks of setup → Factory/Effort → trigger Craftax run → wait → review → hillclimb → publish.  
Observed failure: **cycle 1 only** — capability refusal → `async_checkpoint_waiting_for_event` → wake-less `sleeping` → hillclimb never started.

---

## 3. Corrections to the liveness handoff (do not re-implement the wrong fix)

Reviewed against code 2026-08-08. Keep F-1; reframe F-2 and F-3.

### F-1 — CONFIRMED

`CycleCheckpointed` with no cadence → `async_checkpoint_waiting_for_event` → `SLEEPING` with no producer check (`reducer.py:897-911`). It does not even assert `next_wake_at=None`; it inherits from `BeginCycle`.

**Not** the only wake-less path. Legitimate AC1 paths:

| Path | Producer |
|---|---|
| `async_cycle_all_efforts_awaiting_input` (`:507`) | operator answers `parked_questions` |
| `async_input_requested_cycle_idle` (`:1550`) | same |

**Correct invariant (not the handoff §7 wording):**

```
  wake-less SLEEPING  ⇒  blocker_code  OR  non-empty parked_questions
                       (eventually: typed ResumeCondition with a producer)
```

Shipping “no SLEEPING with null wake and null blocker” would break AC1.

**L2 is safe** on the `CycleCheckpointed` path only — legitimate wake-less waits do not flow through it.

### F-2 — PARTIAL (bigger bug next door)

`leave_safe.py:317-321` equates `async_checkpoint_scheduled` and `async_checkpoint_waiting_for_event`. That only scores the **smoke ladder**. The SDK `leave_safe=True` observed in the cell is:

```python
# packages/intern/contracts.py
leave_safe: Literal[True] = True
```

Nothing in `services/` reads the progress ladder for escalation. Splitting checkpoint kinds is still correct (~3 lines) but does **not** stop production from reporting healthy. Honest `leave_safe` derivation belongs in this refactor.

### F-3 — CONFIRMED finding, wrong mechanism

O4 `scan_async_stall_signals` requires `next_wake_at IS NOT NULL` (`async_workflow_sweeper.py:233-234`) — real blind spot for wake-less rows.

But `_scan_judgment_stalls` **already** selects `next_wake_at IS NULL` (`:322-323`) and then **drops** empty-parked at `:334` (`if not parked: continue`). That `continue` is where the deadlock became invisible.

**Do not** add a third parallel scanner. Invert the existing filter: one query, two dispositions (judgment vs empty-parked). **Never** `BlockAsync` the judgment branch (AC1). Alert-only first for empty-parked; promote to `BlockAsync` after false-positive rate is known.

`AsyncBlockerHandoffContextV1` is **not** drop-in reusable for watchdog blocks (requires action digest). `BlockAsync` + `emit_p1_alert` + spend-ceiling admission pattern **are** reusable.

### Bonus: `sleeping` collapses ≥6 situations

| Situation | Resume |
|---|---|
| Cadence timer | Temporal wake |
| Reconcile wait / re-poll | Temporal wake |
| Actor reply pending | Timer + correlation |
| All Efforts parked | `provide_input` |
| Input requested, idle | human answer (± timer) |
| `waiting_for_event` with nothing | **nothing** ★ |

Same coarseness for `blocked` (budget / actor-miss / infra / handoff) and `executing_cycle` (model vs MCP in flight vs publish).

**Dead enum members (never assigned by reducer):** `CHECKPOINTING`, org `AWAITING_INPUT`, `CANCELLING`, `FAILED`.

---

## 4. Naming scheme — `noun:verb`

Status must answer: **is the agent running, and if not, who may start him again?**

Wire form: `{noun}:{verb}`.

| Today | Proposed `phase` |
|---|---|
| `created` | `lifecycle:not_started` |
| `planning` | `agent:bootstrapping` |
| `executing_cycle` | `agent:running` (+ `agent:tool` / `agent:publishing` when pending IDs set) |
| `sleeping` | `agent:waiting` or `agent:gated` or `world:waiting` (discriminated by resume) |
| `reconciling` | `world:syncing` |
| `awaiting_evidence` | `agent:waiting_evidence` |
| `paused` | `agent:held` |
| `blocked` | `agent:gated` |
| `completed` | `agent:finished` |
| `cancelled` | `agent:cancelled` |
| `failed` | `agent:failed` |
| `checkpointing` / org `awaiting_input` / `cancelling` | **drop** |

Read rules:

```
  agent:{bootstrapping|running|tool|publishing}  ⇒  agent ON
  everything else                                ⇒  agent OFF
  world:*                                        ⇒  agent OFF, platform busy with world

  agent:waiting  ⇒  runtime may resume (timer | WaitOn | parked)
  agent:gated    ⇒  only resolve / timeout / human answer
  agent:held     ⇒  only operator resume
```

`PLANNING` is a bad name: it is bootstrap/replan **effect in flight**, not “agents plan.” Planning belongs to the agent inside `agent:running`.

Keep legacy `AsyncStatus` column for one deprecation cycle; `phase` is additive.

---

## 5. Finer model — three axes

Not a bigger flat enum. Three orthogonal pieces:

```
  phase          — noun:verb lane (derive / project)
  stop_reason    — why agent is off (durable enum)
  resume         — what turns him on (durable dataclass)  ← load-bearing
```

### 5.1 Logical ASCII

```
                 lifecycle:not_started
                        │ start
                        ▼
                 agent:bootstrapping          agent ON (bootstrap invoke)
                        │
                        ▼
                   agent:running ◄── world:syncing ◄── resume
                        │
          ┌─────────────┼──────────────┬─────────────────┐
          ▼             ▼              ▼                 ▼
   agent:waiting   agent:gated   agent:held      world:waiting
   agent OFF       agent OFF     agent OFF       agent OFF
   wake allowed    need resolve  need resume     run/reply
          │             │              │                 │
          └─────────────┴──────┬───────┴─────────────────┘
                               ▼
                         world:syncing
                               │
                               ▼
                         agent:running | agent:waiting_evidence
                                           │
                                           ├─► agent:waiting
                                           ├─► agent:finished
                                           └─► agent:gated

  every OFF edge (non-terminal) carries:
    stop_reason + resume{kind, wake_at|producer, watchdog_at}

  invariant:
    agent:waiting  ⇒  wake_at OR WaitOn(producer) OR parked interaction_ids
    agent:gated    ⇒  blocker_code OR human_answer resume
    never          ⇒  agent OFF with resume.kind = never (except terminals)
```

### 5.2 Replace naked `waiting_for_event`

When ending a tick, the agent/runtime must choose an explicit stop:

| Primitive | Effect |
|---|---|
| SleepUntil(t) | `agent:waiting`, resume TIMER |
| WaitOn(producer, timeout) | `agent:waiting` or `world:waiting`, always a timeout backstop + named producer |
| Block(code, rationale) | `agent:gated`, Sync/approval path |
| Judgment park | `agent:gated` or `agent:waiting` + HUMAN_ANSWER (AC1: do not BlockAsync org) |

**Delete** `CycleCheckpointed` with `next_wake_at=None` and no subscription / park / block.

---

## 6. Synth Style implementation pack

Style citations: `tigerstyle.md` → Assertions, Limits, Contracts, Degradation, Authority; `synthstyle.md` → dataclasses/enums, fail-fast, one authoritative meaning for status/phase.

### 6.1 Types (packages/intern — pure domain)

```python
class PhaseNoun(StrEnum):
    LIFECYCLE = "lifecycle"
    AGENT = "agent"
    WORLD = "world"

class PhaseVerb(StrEnum):
    NOT_STARTED = "not_started"
    BOOTSTRAPPING = "bootstrapping"
    RUNNING = "running"
    TOOL = "tool"
    PUBLISHING = "publishing"
    WAITING = "waiting"
    WAITING_EVIDENCE = "waiting_evidence"
    HELD = "held"
    GATED = "gated"
    FINISHED = "finished"
    CANCELLED = "cancelled"
    FAILED = "failed"
    SYNCING = "syncing"  # world only

class StopReason(StrEnum):
    NONE = "none"
    CADENCE_TIMER = "cadence_timer"
    JUDGMENT_PARKED = "judgment_parked"
    NO_RUNNABLE_EFFORT = "no_runnable_effort"
    TOOL_IN_FLIGHT = "tool_in_flight"
    PUBLISH_IN_FLIGHT = "publish_in_flight"
    ACTOR_REPLY_PENDING = "actor_reply_pending"
    EXTERNAL_EXECUTION_ACTIVE = "external_execution_active"
    EVIDENCE_FINALIZING = "evidence_finalizing"
    OPERATOR_HOLD = "operator_hold"
    BUDGET_EXHAUSTED = "budget_exhausted"
    CAPABILITY_GATE = "capability_gate"
    INFRASTRUCTURE_FAILURE = "infrastructure_failure"
    TERMINAL = "terminal"

class ResumeKind(StrEnum):
    TIMER = "timer"
    HUMAN_ANSWER = "human_answer"
    OPERATOR_COMMAND = "operator_command"
    EXTERNAL_EVENT = "external_event"
    NEVER = "never"  # terminal only

@dataclass(frozen=True, slots=True)
class RuntimePhase:
    noun: PhaseNoun
    verb: PhaseVerb

    def __post_init__(self) -> None:
        if (self.noun, self.verb) not in _LEGAL_PHASES:
            raise ValueError("runtime_phase_pair_illegal")

    @property
    def wire(self) -> str:
        return f"{self.noun.value}:{self.verb.value}"

    @property
    def agent_on(self) -> bool:
        return self.noun is PhaseNoun.AGENT and self.verb in {
            PhaseVerb.BOOTSTRAPPING,
            PhaseVerb.RUNNING,
            PhaseVerb.TOOL,
            PhaseVerb.PUBLISHING,
        }

@dataclass(frozen=True, slots=True)
class ResumeCondition:
    kind: ResumeKind
    wake_at: datetime | None = None
    watchdog_at: datetime | None = None
    interaction_ids: tuple[str, ...] = ()
    effort_ids: tuple[str, ...] = ()
    external_ref: str | None = None

    def __post_init__(self) -> None:
        if len(self.interaction_ids) > ASYNC_PARKED_QUESTIONS_MAX:
            raise ValueError("resume_interaction_ids_limit_exceeded")
        if len(self.effort_ids) > ASYNC_EFFORT_FANOUT_MAX:
            raise ValueError("resume_effort_ids_limit_exceeded")
        if self.kind is ResumeKind.TIMER and self.wake_at is None:
            raise ValueError("resume_timer_wake_at_missing")
        if self.kind is ResumeKind.HUMAN_ANSWER and not self.interaction_ids:
            raise ValueError("resume_human_answer_ids_missing")
        if self.kind is ResumeKind.EXTERNAL_EVENT and not self.external_ref:
            raise ValueError("resume_external_ref_missing")
        if self.kind is ResumeKind.NEVER and (
            self.wake_at is not None
            or self.interaction_ids
            or self.effort_ids
            or self.external_ref is not None
        ):
            raise ValueError("resume_never_must_be_empty")

@dataclass(frozen=True, slots=True)
class AgentBackpressure:
    """Why the agent is off, and what turns him on.

    Absent (None on AsyncState) only when agent ON or lifecycle:not_started
    before first admit — never when agent OFF and non-terminal.
    """

    stop_reason: StopReason
    resume: ResumeCondition
```

On `AsyncState`: `backpressure: AgentBackpressure | None = None`.  
**Do not** persist `phase` — derive it. One authoritative meaning (*synthstyle*: status/phase from one canonical model).

Named helpers only (no naked SLEEPING constructors):

- `waiting_on_timer(...)`
- `waiting_on_judgment(...)`
- `waiting_on_external(...)`
- `gated(...)`
- `held(...)`
- `terminal(...)`

### 6.2 Invariants (pair-assert write + read)

Per *tigerstyle* → Assertions: assert positive and negative space; pair before persist and after rehydrate; split compound asserts.

```python
def assert_async_state_invariants(state: AsyncState) -> None:
    phase = derive_phase(state)

    assert phase.wire in {p.wire for p in LEGAL_PHASES}
    assert state.generation >= 0
    assert state.cycle_number >= 0

    if phase.agent_on:
        assert state.backpressure is None
        assert state.blocker_code is None
        return

    if phase.verb in {
        PhaseVerb.FINISHED,
        PhaseVerb.CANCELLED,
        PhaseVerb.FAILED,
    }:
        assert state.backpressure is not None
        assert state.backpressure.resume.kind is ResumeKind.NEVER
        return

    if phase.verb is PhaseVerb.NOT_STARTED:
        return

    # agent/world OFF, non-terminal
    bp = state.backpressure
    assert bp is not None
    assert bp.stop_reason is not StopReason.NONE
    assert bp.resume.kind is not ResumeKind.NEVER

    if phase.noun is PhaseNoun.AGENT and phase.verb is PhaseVerb.WAITING:
        assert (
            bp.resume.wake_at is not None
            or bp.resume.interaction_ids
            or bp.resume.external_ref is not None
        )

    if phase.verb is PhaseVerb.GATED:
        assert (
            state.blocker_code is not None
            or bp.resume.kind is ResumeKind.HUMAN_ANSWER
        )

    if state.pending_action_id is not None:
        assert phase.verb is PhaseVerb.TOOL
    if state.pending_actor_message_id is not None:
        assert phase.verb is PhaseVerb.PUBLISHING
```

**Pair sites:**
1. End of `_applied` (every successful reduction).
2. `_async_state` rehydrate after JSON parse (`runtime_authority`).
3. Property test over reducer command surface: every `APPLIED` state passes.

On load, prefer `raise ValueError("async_state_invariant_violated:…")` with a stable code over silent projection of corrupt rows (*Degradation*: absent ≠ failed).

### 6.3 Projection API additions

On `AsyncRuntimeResponse` (additive; `_StrictContract` ⇒ **SDK bump same train**):

- `phase: str` (wire `noun:verb`)
- `stop_reason: StopReason | None`
- `resume: ResumeConditionResponse | None`
- `active_effort_id: str | None` (already in reducer state; not projected today)
- `outcome: RuntimeOutcome | None` (durable today; not projected)
- `leave_safe: bool` — **stop** `Literal[True]`; derive honestly (agent on, or waiting with named resume and no hard gate, etc.)

Keep `status: AsyncStatus` for one deprecation cycle.

### 6.4 Persistence

`backpressure` / `stop_reason` / `resume` live in `runtime_state` JSON (same pattern as AC3 cycle cursor — `runtime_authority` “JSON payload; no new column”). No alembic required for the first land.

Compat: missing key → derive from legacy fields + **INFO**-level operator-visible log (`COMPAT:`). Unknown shape fails closed. Flag for removal.

Do **not** widen DB CHECK on `status` with new strings; new vocabulary never hits that column until a deliberate later migration.

### 6.5 Sweeper

1. Invert `_scan_judgment_stalls`: empty-parked → `emit_p1_alert("intern_async_sleeping_without_wake")` (own grace env); parked → existing judgment alert; never BlockAsync judgment.
2. After `resume.watchdog_at` exists: page on `watchdog_at < now` / `stop_reason` enums — one authority, no JSON-shape reconstruction (*Authority and duplication*).
3. Promote empty-parked to `BlockAsync` only after alert volume is known; copy `_admit_spend_ceiling_block` including existing-blocker-code idempotency guard.
4. Longer-term: stall clock on `state_generation` / `last_event_sequence`, not `updated_at` (owner-event observations reset `updated_at` today).

### 6.6 leave_safe smoke ladder

Still split `async_checkpoint_scheduled` vs `async_checkpoint_waiting_for_event` in `leave_safe.py` — independently correct for smoke budgets. Expect smoke runs that only emit waiting-for-event to hit the 180s `CHECKPOINT_OBSERVED` budget. Not load-bearing for production escalation.

---

## 7. Sequencing

| Step | Change | Status |
|---|---|---|
| 0 | Types + `derive_phase` + project `phase` / `active_effort_id` / `outcome` | **Landed** |
| 1 | Invert judgment scanner empty-parked → alert | **Landed** (+ gated-without-resume pages) |
| 2 | Honest `leave_safe` bool | **Landed** |
| 3 | Persist `AgentBackpressure` on reductions; invariant asserts | **Landed** (+ COMPAT promote) |
| 4 | Ban / rewrite bare `waiting_for_event` → gated without resume | **Landed** |
| 5 | Sweeper watchdog bound; BlockAsync promotion deferred | **Partial** (alert-only; no auto-block) |
| 6 | Deprecate dead `AsyncStatus` members; leave_safe.py split | **Landed** (markers; enum values remain for DB CHECK) |
| 7 | L1 subscription vocabulary; L3 model-declared blockers + rate limits | **Open** |

**Minimum defensible:** steps 0–2 ✅  
**Class dead at write time:** steps 3–4 ✅  
Capability sibling removes *this* trigger; synthetic reducer invariant tests remain the durable lock.

---

## 8. Tests

- **Invariant property:** every APPLIED reduction passes `assert_async_state_invariants`.
- **Negative space:** `ResumeCondition(kind=TIMER, wake_at=None)` raises; AC1 judgment paths still wake-less **with** HUMAN_ANSWER resume / parked ids — **no** org BlockAsync.
- **Illegal sleep:** no path yields `agent:waiting` with empty producers.
- **L2 / checkpoint:** no cadence + no park → gated or WaitOn, not bare sleeping.
- **Sweeper:** empty-parked past grace → new alert once; parked → judgment alert only, never BLOCKED; already-blocked not re-signalled.
- **Contract:** `leave_safe` is False for wake-less empty-parked and for gated; True only when honestly healthy.
- **Leave-safe unit:** `waiting_for_event` alone ≠ `CHECKPOINT_OBSERVED`.
- **Compat:** old JSON without backpressure derives + logs; does not silently look healthy when resume missing.

---

## 9. Related open defects (same train of thought)

From Bugbot on dirty tree (not this refactor, but feeds the deadlock):

- Capability **prompt availability** ignores arg-scoped admissibility the gate allows → agent told tools are blocked → pushes toward `waiting_for_event` (`packages/intern/capability_availability.py`). Fix with capability sibling / shared verdict source (*Authority*).
- `on_event` awaited inline in Codex event loop (`packages/horizons/actors/runner.py`) — can stall turns.
- Internal Codex activity SSE: worker token + any run/execution id without org binding (security medium).

---

## 10. File touch list

| Area | Paths |
|---|---|
| Domain types / invariants | `packages/intern/async_/state.py`, `packages/intern/async_/phase.py` |
| Reducer choke point | `packages/intern/async_/reducer.py` |
| Contracts / OpenAPI | `packages/intern/contracts.py`, generated OpenAPI / synth-ai vendored |
| Prompt contract | `packages/intern/prompts.py` |
| Projection / rehydrate | `services/intern/async_backpressure.py`, `runtime_authority.py`, `product.py` |
| Sweeper | `services/intern/async_workflow_sweeper.py` |
| Smoke ladder | `packages/intern/leave_safe.py` |
| Tests | `tests/units/test_intern_async_phase.py`, `test_intern_runtime_core.py`, `test_leave_safe_fail_fast.py`, `tests/integration/test_intern_async_stall_signals_postgres.py` |
| Spec link | this handoff + `notes/specifications/tanha/current/systems/intern/runtime_authority.md` |

---


## 12. Open-item completion (same day)

| Item | Landed |
|---|---|
| L1 named `WaitOn` | `AsyncWaitOn` + `WaitProducer`; checkpoint `wait_on`; reducer resolvability; prompt; agent decode |
| L3 model-declared blockers | `InternAgentResultKind.BLOCKER`; adapter → `block`; rate-limit one model block when already gated; prompt |
| empty-parked → `BlockAsync` | Env `INTERN_ASYNC_EMPTY_PARKED_BLOCK_ENABLED` default **OFF**; shared `_admit_internal_block`; alert always |
| `watchdog_at` at stop sites | `watchdog_after` + `reduce(..., now=)`; runtime_authority passes wall clock; grace env optional |
| Capability sibling (min) | `ARGUMENT_SATISFIABLE_SCOPES`; three-bucket prompt availability (now / via args / blocked) |
| Wire enums | `RuntimePhaseWire` / `StopReason` / `ResumeKind` / `WaitProducer` on contracts + research OpenAPI + synth-ai lockstep |

**Verification:** `141` unit tests green across phase / runtime core / leave_safe / capability_availability.

## 11. Done when

1. SDK/UI can read `phase` + `resume` and know why the agent is off and what resumes him. ✅
2. `leave_safe` is not a compile-time `True`. ✅
3. Empty-parked wake-less sleep **and** gated-without-resume page within a grace window. ✅ (finish-pass)
4. No reachable APPLIED state violates the resume invariant; COMPAT rehydrate promotes rather than wedges. ✅ (finish-pass)
5. Bare `async_checkpoint_waiting_for_event` without producer is unconstructible (gates instead). ✅
6. Judgment-parked orgs are never auto-`BlockAsync`'d. ✅
7. Style: dataclasses/enums/assertions on the domain path; wire `phase`/`stop_reason` still `str` for OpenAPI compat (tighten later). ⚠️ partial
8. Prompt does not teach null wake without judgment. ✅ (finish-pass)

**Previously deferred — now landed** (see §12). Still optional: enable empty-parked BlockAsync in prod after alert volume; drop dead `AsyncStatus` from DB CHECK; broader capability `requires_project` audit; frontend `smr_openapi.yaml` regen if needed.
