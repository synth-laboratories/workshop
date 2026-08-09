# Handoff: Async Intern liveness — indefinite sleep reports as healthy

**Date:** 2026-08-08
**Observed on:** acceptance cell `A-Craftax`, run `20260808T144218Z`, slot6, org `878975dd-1b3a-4f6b-a328-f58e954db701`
**Subsystems:** `packages/intern/async_/reducer.py`, `packages/intern/leave_safe.py`, `services/intern/async_workflow_sweeper.py`
**Style bar:** `backend/tigerstyle.md`
**Sibling docs:**
- `INTERN_CAPABILITY_BINDING_HANDOFF_2026-08-08.md` — *why* the Intern got stuck. **This doc is about why nothing noticed.** They are independent; either can land first.
- `INTERN_ASYNC_RUNTIME_PHASE_REFACTOR_HANDOFF_2026-08-08.md` — structural fix: `noun:verb` phase, typed resume/backpressure, Synth Style invariants, corrected F-2/F-3 sequencing. Prefer that doc when implementing.

---

## 1. The one-sentence problem

An Async Intern that sleeps with no wake time, no blocker, and nothing pending is **permanently
deadlocked but reports as healthy** — and every mechanism built to catch a stalled runtime is
structurally blind to it.

Observed state, held for the full 90-minute cell timeout:

```
status        = sleeping
next_wake_at  = None       ← nothing scheduled; it will never self-wake
blocker       = None       ← it does not consider itself blocked
leave_safe    = True       ← so no escalation or handoff path fires
cycle_number  = 1 (of 24)  ← the cycle budget will never be consumed
updated_at    = 14:47:16Z  ← frozen; zero state change thereafter
spend         = $0.15      ← so no budget ceiling will ever trip
```

Nothing in the product ended this. The acceptance harness killed it at `--timeout-seconds`. **In
production it would sit there indefinitely**, consuming an org's one async runtime slot, until a
human happened to look.

---

## 2. Root cause — three independent findings

### F-1. Absence of a proposed cadence *is* the definition of "waiting for an event"

`packages/intern/async_/reducer.py:896-910`:

```python
if cycle_next_wake_at is None:
    return _applied(
        state, command,
        "async_checkpoint_waiting_for_event",
        status=AsyncStatus.SLEEPING,
        ...                                   # note: no next_wake_at, no blocker_code
    )
```

The model did not propose a wake time, so the reducer concluded **an external event will wake it**.
Nothing verifies that any such event has a producer.

The three sleep outcomes:

| Reducer path | Line | Wakes when | Valid if… |
|---|---|---|---|
| `async_cycle_effort_advanced` | `:874` | immediately — more work queued | always |
| `async_checkpoint_scheduled` | `:926` | `next_wake_at` (clamped) | always |
| `async_checkpoint_waiting_for_event` | `:901` | **never, unless someone else acts** | *a producer exists — never checked* |

The third is the only one whose validity depends on a fact about the world, and it is the only one
that asserts nothing.

### F-2. Leave-safe counts the deadlock as progress

`packages/intern/leave_safe.py:317-321`:

```python
if (
    "async_checkpoint_scheduled" in event_kinds
    or "async_checkpoint_waiting_for_event" in event_kinds
):
    current = _at_least(LeaveSafeProgress.CHECKPOINT_OBSERVED)
```

The two checkpoint kinds are treated as equivalent. **One carries a wake time; the other is defined
by its absence.** So the exact event meaning "I am deadlocked" advances the runtime toward
leave-safety — which is why `leave_safe=True` on a permanently stuck Intern.

This is wrong on its own terms, independent of everything else in this document, and is the smallest
correct change in it.

### F-3. The stall detector explicitly filters out this case

`services/intern/async_workflow_sweeper.py:208-256` (`scan_async_stall_signals`, WP3 O4) is the
purpose-built stalled-runtime detector. Its query:

```python
SmrInternAsyncAssignment.next_wake_at.isnot(None),      # ← :233
SmrInternAsyncAssignment.next_wake_at < stale_deadline, # ← :234
```

**It only catches runtimes that missed a deadline.** A runtime with *no* deadline can never become
past-due, so it is invisible to the one scanner built to find it.

This blind spot has already produced a second bug and been patched by special case — `:92-94`:

> `WP-AC / AC3: an Effort parked on judgment does not stall the org, but ... next_wake_at — so the
> O4 stale-wake scan cannot see it. Alert separately.`

…leading to `_scan_judgment_stalls` as a parallel scanner. **That is the second instance of the same
root defect being routed around rather than fixed.** A third will follow unless the predicate
changes from "missed its deadline" to "cannot make progress."

---

## 3. The escape hatch already exists — nothing reaches it

The blocker system is complete and well-designed:

- `packages/intern/contracts.py:890-984` — `AsyncBlockerResponse`, `AsyncBlockerHandoffContextV1`
  (carries `summary`, `rationale`, `required_operator_capability`), `AsyncBlockerOpenSyncRequest`/
  `Response`, `AsyncBlockerHandoffReceiptV1`, `AsyncBlockerResolveRequest`.
- **A blocked Async Intern can open a Sync session with a human**, with context and a receipt chain.
- `blocker_code` is first-class state (`async_/state.py:296`, `:377`) and reducer paths already set
  it: `async_maximum_cycles_exhausted` (`reducer.py:479`), `intern_actor_reply_missing` (`:1048`,
  `:1192`), and commands can carry it (`:1124`, `:1766`).
- The sweeper **already admits `BlockAsync`** for spend ceilings (docstring `:27-31`), so there is a
  proven path for a background pass to mark a runtime blocked.

This is exactly the right response to "I need a Factory binding and cannot create one." The
machinery is not missing. **There is simply no path from this state into it.**

---

## 4. First principles

The invariant that should hold:

> **A runtime may sleep indefinitely only if it can name a producer that will wake it.**

"Waiting for an event" is a *claim about the world*. Today it is *inferred from a missing field*.
Every finding in §2 follows from that one substitution — a claim that is never checked, a
progress ladder that credits it, and a detector that can only see deadlines.

Per `tigerstyle.md` → *Limits*: "Put a limit on everything. All loops, queues, retries, and batches
must have a fixed upper bound… Where a loop is intentionally unbounded, document why." **An
unbounded sleep is an unbounded loop with extra steps**, and this one is neither bounded nor
documented.

Per `tigerstyle.md` → *Degradation and observability*: "Do not collapse **absent** and **failed**
without a contract when callers must distinguish them." `next_wake_at = NULL` currently means both
"no cadence needed" and "no cadence possible."

---

## 5. Design — four layers

Ordered weakest to strongest. They compose; pick a subset deliberately.

### L1 — Make the wait name its producer *(the real fix)*

`async_checkpoint_waiting_for_event` stops being a fallback and becomes a claim the checkpoint must
substantiate: **what** is it waiting on — `run_id`, `effort_id`, an operator reply, a mailbox
message? If the reducer cannot resolve a live producer for that subscription, the outcome is a
blocker, not a sleep.

This makes the illegal state unrepresentable rather than merely detectable. It is also the largest
change, and it needs a decision about the subscription vocabulary — keep it a closed enum
(*tigerstyle* → *Contracts*: "Configuration and mode switches must draw from a defined vocabulary").

### L2 — Invert the default *(cheapest, fail-closed)*

At `reducer.py:896`, `cycle_next_wake_at is None` → **blocker**, not indefinite sleep. The model must
either propose cadence or declare a blocker explicitly. Suggested code:
`async_no_cadence_and_no_subscription`.

Roughly 15 lines. Closes the production hole immediately. Risk: any legitimate wake-less wait that
exists today becomes a blocker — **audit for those first** (`grep` the reducer's callers and check
whether mailbox-driven waits rely on this path). If legitimate cases exist, L2 must wait for L1.

### L3 — Give the agent a voice

Our Intern's checkpoint said *"Await an authorized Factory binding and active Effort."* It knew
exactly what it needed. What it could not say was **"and nobody can give me one."**

Let the model declare a blocker as a first-class checkpoint outcome, and make a capability refusal it
cannot route around surface that as an available move. This pairs directly with R-5/R-6/R-7 of the
sibling handoff: the refusal tells the agent what would satisfy the gate, the prompt tells it what it
is allowed to do, and if neither yields a path forward it escalates instead of sleeping.

### L4 — Watchdog backstop *(nearly free — the machinery exists)*

Extend `scan_async_stall_signals` (`async_workflow_sweeper.py:208`) with a second predicate:

```
status = sleeping
AND next_wake_at IS NULL
AND blocker_code IS NULL
AND pending_instruction_count = 0
AND awaiting_actor_reply_message_id IS NULL
AND updated_at < now() - <grace>
```

→ `emit_p1_alert("intern_async_sleeping_without_wake")`, and admit `BlockAsync` exactly as the spend
path already does.

This runs on an interval the Temporal worker already invokes, reuses an alert helper and an
admission path that already exist, and — critically — **it is the only layer that would have caught
this bug without foresight.** It also subsumes `_scan_judgment_stalls`, so F-3's special case can
eventually be deleted rather than joined by a third.

Prefer generalizing the existing scanner over adding a third parallel one (*tigerstyle* →
*Authority and duplication*).

### Bonus signal, free with any layer

Cycle 1 completed with **zero durable resource change** (no Project, no Factory, no file write) *and*
no wake time. "Slept having accomplished nothing, with no way to be woken" needs no subscription
model to detect and is a strictly stronger signal than either condition alone.

---

## 6. Recommended sequencing

1. **F-2 fix** (`leave_safe.py:319`) — split the two checkpoint kinds. Independently correct,
   ~3 lines, no design decisions.
2. **L4** — generalize the sweeper predicate. Closes the production hole with existing machinery.
   Ship with L2 or alone.
3. **L2** — after auditing for legitimate wake-less waits. If any exist, skip to L1.
4. **L3** — rides along with the capability work in the sibling handoff.
5. **L1** — the contract, once the subscription vocabulary is settled.

**1 + 2 alone** mean no deadlock can report healthy and anything that slips through is caught within
the grace window. That is the minimum defensible state.

---

## 7. Verification

**Reproduce** — this is deterministic today, given the sibling handoff's bug:

```bash
cd ~/Documents/GitHub/testing/acceptance_tests/intern_247_launch
./bin/run_matrix.sh --slot slot6 --cell A-Craftax --force-blocked \
  --stream-intern --stream-codex --continue-on-error --timeout-seconds 900
```

Watch `bin/tail_intern.py` for `⏸ async_checkpoint_waiting_for_event`, then confirm via the SDK that
`next_wake_at is None`, `blocker is None`, `leave_safe is True`. Use a short timeout — there is
nothing to wait for.

Note the dependency: once the capability bug is fixed the Intern will proceed past this point, so
**a synthetic reducer test is the durable reproduction**, not the acceptance cell.

**Tests to add:**

- Reducer unit: a checkpoint with no proposed cadence and no resolvable subscription yields a
  blocker, not `SLEEPING` with `next_wake_at=None`.
- Leave-safe unit: `async_checkpoint_waiting_for_event` alone does **not** reach
  `CHECKPOINT_OBSERVED`, while `async_checkpoint_scheduled` does. (Existing coverage lives around
  `tests/units/test_intern_runtime_core.py`.)
- Sweeper: a sleeping assignment with `next_wake_at IS NULL` past the grace window is detected and
  blocked. Assert the *absence* case explicitly — F-3 is a bug of omission and only a negative-space
  test prevents its return.
- Invariant test worth writing once: **no reachable reducer path produces
  `status=SLEEPING AND next_wake_at IS NULL AND blocker_code IS NULL`.** That single assertion is
  what keeps this class dead.

---

## 8. Constraints

- **Do not push.** Land locally; Josh decides when it goes out.
- **Do not make a stuck runtime look busy to satisfy a detector.** The fix is to *report* the
  deadlock, never to synthesize a wake time that papers over it.
- Blocker escalation opens a **Sync session with a human** — treat the volume as a product decision,
  not an implementation detail. Alert first (L4), escalate second, once the false-positive rate is
  known.
- `leave_safe` is load-bearing for the I2 gate; changing F-2 may move I2's behavior. Check before
  assuming it is inert.

## 9. Related

- `INTERN_CAPABILITY_BINDING_HANDOFF_2026-08-08.md` — the capability deadlock that produced this
  state. Fixing it removes *this* trigger but not the class.
- `grader_pass` has never fired on any cell (0 passes / 27 attempts). Every prior failure was
  infrastructure; these two handoffs are the first genuine product findings from the suite.
- Referenced plans: `plans/smr/intern_async_24_7_change_scope.md` (WP1 L3/L6, WP3 O4/O5, WP2 S1) —
  the O4 stall-scan design is where F-3's filter was introduced.
