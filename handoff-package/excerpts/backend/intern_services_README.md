# Intern infrastructure services

Infrastructure bindings for the pure Intern package.

# See: intern_async_24_7_change_scope.md (WP-R0 / WP-R1 R1.2 / R1.3 / R1.6 / R1.7 / WP-AC)

## Architecture (WP-R1 R1.7)

```
  Client / MCP / FE
        |
        v
  app/api  (HTTP glue only)
        |
        v
  services/intern/                 <-- admission + authority live HERE
    product.py                     command admission, hard-stop, budgets
    mailbox_repository.py          ordered commands / events
    runtime_authority.py           pause/cancel → release lease AND box
    sticky_host_leases.py          Async sticky exe.dev lease + VM teardown
    lease_reaper.py                dead WF → release lease AND box (R1.6)
    effects.py / context_compiler  day + month cost budgets
    ...
        |
        | Temporal activities / runtime_actor_client
        v
  intern-and-smr-runtime           <-- runners live HERE (peer process)
    services/intern/runners/       hosts, actor service, control routes
      hosts.py                     _open_host_session: create_sandbox
                                   (idempotent) + acquire sticky lease
                                   Resume after pause/reaper reacquires here.
        |
        v
  packages/persistent_compute      ExeDevClient / leases (pure substrate)
  packages/intern                  pure reducers / contracts (WP-AC parked_questions)
```

**Org sticky exe.dev (Issue #1 / R1.2 / R1.6):** Sync and Async share one
org-scoped sticky box (`org-async:{org}`). Idle cycles do **not** free the
lease. **Pause** and the **liveness reaper** free the **lease** for metering;
host **destroy** stays off until `INTERN_EXE_DEV_FILESTORE_BACKUP_READY=1`
(Wasabi filestore backup). Resume reacquires on the next `_open_host_session`.
Per-actor guest workspaces isolate Sync/Async files on the shared VM.

**Day / month budgets:** Async assignments carry `maximum_daily_cost_cents` and
`maximum_monthly_cost_cents`; effects gate on summed `intern_execution` LLM
facts plus org-scoped `intern_sticky_host` idle burn
(`INTERN_EXE_DEV_HOST_CENTS_PER_HOUR`, default 50¢/hr; see
`sticky_host_leases.py`). When a ceiling binds, Async admits `BlockAsync` with
`async_daily_budget_exhausted` / `async_monthly_budget_exhausted` (effect path
+ sweeper `enforce_async_spend_ceilings`) so the projection shows the blocker
(WP2 S1).

**WP-AC parked questions:** Async judgment asks are parked **per Effort** in
`parked_questions` and survive `BeginCycle`. Org runtime stays runnable while
other Efforts progress. `provide_input` resolves the exact `interaction_id`.

**WP-AC / AC3 multi-Effort execution:** a cycle now advances a *slate* of
Efforts, not one anonymous unit of work.

- **Selection** — `select_cycle_efforts` (pure, in `packages/intern/async_/
  reducer.py`) drops parked / `awaiting_input` Efforts, then orders the rest by
  `(last_advanced_cycle, candidate order)` — round-robin on least-recently-
  advanced — and truncates to `INTERN_ASYNC_EFFORT_FANOUT_MAX` (default 3, hard
  cap 16). No clocks, no randomness: safe under Temporal replay.
- **Execution** — the slate runs **sequentially** inside the cycle. The head
  Effort gets a `RunAsyncCycle` effect carrying `effort_id`; its checkpoint
  chains the next slot (`async_cycle_effort_advanced`); the last slot sleeps the
  cycle on the *earliest* cadence any slot asked for. Temporal remains the sole
  durability authority — one effect per tick, single in-flight MCP slot.
- **Park** — an agent result with a `judgment` object becomes `request_input`
  (`effect_adapters._async_judgment_observation`). The parked Effort surrenders
  its slot and the cycle continues on the next queued Effort
  (`async_input_requested_cycle_continued`) without the org status leaving the
  runnable lane. When no Effort is runnable the runtime goes `sleeping`, never
  `awaiting_input` / `blocked` (`async_cycle_all_efforts_awaiting_input`).
- **Resolve** — `provide_input` makes exactly that Effort runnable and puts it
  at the head of the next slate.
- **Attribution** — `active_effort_id` rides the effect payload into
  checkpoints, judgment asks, and `InternMcpExecutionContext.effort_id`; a tool
  call naming a different Effort is refused
  (`capability_effort_binding_conflict`).
- **Ops** — the sweeper alerts `intern_async_judgment_stalled` when *every*
  Effort of an assignment has been parked past
  `INTERN_ASYNC_JUDGMENT_STALL_GRACE_SECONDS` (default 6h); a single parked
  Effort is normal and is deliberately not paged on.

**Sync session-sticky (WP-R1 R1.3):** turn N reuses the turn-1 host.
`warm_session` defaults (and is forced) `true` for Sync via
`INTERN_ACTOR_EXECUTION_JSON` / `InternActorProfileResolver`. exe.dev keys
`create_sandbox` on `run_id` (= actor_id) without an Async org lease; Daytona
sets `retain_sandbox=profile.warm_session` and restores `sandbox_id` from
checkpoint `result_output` across turns.

**Modal (open Q #1):** Sync sticky Modal adapter remains fail-closed
(`intern_actor_modal_host_adapter_pending`). Reuse the existing Modal stack
later vs a new adapter — do not block exe.dev/Daytona Sync stickiness on this.
See change-scope **Open questions (remaining) #1**.

Historical Daytona/SMR-leaning diagrams:
`notes/plans/smr/intern_architecture.html` — see the banner there; this README
and `intern_async_24_7_change_scope.md` are the living sources.

## Module map

- `product.py`: PostgreSQL product resource and command admission service.
- `mailbox_repository.py`: explicit HTTP/domain/ORM/Temporal conversions plus
  authoritative command admission, recovery polling, and ordered event pages.
- `runtime_authority.py`: atomic state/event/effect persistence boundary;
  pause/cancel call sticky lease+host release (WP-R1 R1.2).
- `effects.py`: bounded tick claims, capability fencing, and effect receipts;
  day/month cost budget gates; shared `sum_async_runtime_spend_cents` for
  capability evaluate + spend-ceiling BlockAsync.
- `async_workflow_sweeper.py`: restart dead Async WFs (WP1 L3), stall alerts
  (WP3 O4), `enforce_async_spend_ceilings` → BlockAsync (WP2 S1), and
  `renew_async_capability_grants` (WP3 O5).
- `sync_capability_grant_expiry.py`: the Sync half of grant-expiry hygiene
  (WP3 O5) — a *scan*, not a sweeper. Sync grants renew on operator contact via
  `admit_runtime_command`, so the platform owes only the alert that contact has
  stopped and `capability_expired` is now denying silently. Same background
  interval as the sweeper and the lease reaper (`worker_main`).
- `async_cadence_authority.py`: the one place the Async loop's time-shaped knobs
  are resolved from the environment — Effort fan-out (AC3), the `next_wake_at`
  floor (L6), the ops-retention window (L8), and the parked terminal poll (L5).
  They live here rather than in `packages/intern/async_/*` because that tree is
  a Temporal sandbox passthrough, where a module-level env read would *succeed*
  and bake divergent values into replay; resolved values are threaded into the
  pure functions as arguments. A knob the workflow **body** reads does not
  belong here — it travels in `InternWorkflowInput` instead. Existence rationale
  and a ready-made fast-soak block: `synth-dev/config/instances/
  async_24_7_fast_soak.env.example`.
- `effect_adapters.py`: trust-boundary codecs for agent, evidence, and MCP results.
- `runtime_actor_client.py`: Temporal-side typed client for durable Intern actors
  hosted by the Intern peer runner in `intern-and-smr-runtime`; it never launches
  Codex or materializes auth.
- `runners/`: Intern peer runners (hosts, actor service, control routes) colocated
  in `intern-and-smr-runtime` beside SMR run/swarm runners.
- `sticky_host_leases.py`: Async sticky exe.dev lease acquire/renew/release;
  `release_async_sticky_lease_and_host` frees lease **and** box (pause + reaper).
  Default store is durable Postgres (`postgres_host_lease_store.py`).
  WP2 S2: renew / acquire-reuse / release emit `intern_sticky_host` idle-burn
  facts (`INTERN_EXE_DEV_HOST_CENTS_PER_HOUR`).
- `lease_reaper.py`: liveness-tied reaper — dead Async WF → free lease and box
  (WP-R1 R1.6).
- `runtime_actor_store.py`: PostgreSQL actor binding, execution admission,
  replay, generation supersession, and result receipts.
- `agent_runtime.py`: legacy direct Synth API-key adapter for non-actor callers;
  it is not loaded by the Intern Temporal activities.
- `context_compiler.py`: immutable, token-bounded model snapshots assembled from
  generation-fenced state, policy, event, resource, and MCP catalog records.
- `event_outbox.py`: transactional Redis delivery intents and bounded retries.
- `evidence_store.py`: PostgreSQL terminality plus verified Wasabi archive receipts.
- `mcp_gateway.py`: policy-checked bounded MCP JSON-RPC transport.
- `manderqueue_adapter.py`: idempotent actor-message staging and cursor-based
  actor ingress for a bound Run; never the client mailbox.
- `redis_runtime.py`: non-authoritative projections, wakeups, and tick claims.
- `recovery.py`: bounded repair of abandoned invocation receipts, missing
  bootstrap ``start`` commands for runtimes stuck at ``created``, and missing
  Async evidence wakes; it enqueues commands and never mutates product state.
- `runtime_events.py`: PostgreSQL replay and Redis-assisted SSE tailing.
- `workflow_client.py`: idempotent Temporal starts and signals.

PostgreSQL and Temporal are correctness dependencies. Redis is explicitly
degradable: failures are logged with runtime context and recovery proceeds from
PostgreSQL/Temporal state.

Intern model, timeout, and host policy is resolved only by the Intern peer runner
from `INTERN_ACTOR_EXECUTION_JSON`. Async is sticky **exe.dev only**. Sync accepts
sticky exe.dev (preferred), daytona, or modal (modal adapter pending). The Temporal
worker needs its Intern job types, the private `intern-and-smr-runtime` URL, and
`SMR_WORKER_API_KEY`; it contains neither Node nor Codex and receives only an opaque
`intern-chatgpt:{org_id}` auth reference.

```json
{
  "sync": {
    "model": "gpt-5.6-terra",
    "host_kind": "exe_dev",
    "timeout_seconds": 120,
    "reasoning_effort": "medium",
    "sandbox_mode": "read-only",
    "warm_session": true,
    "checkpoint_required": false
  },
  "async": {
    "model": "gpt-5.6-terra",
    "host_kind": "exe_dev",
    "timeout_seconds": 300,
    "reasoning_effort": "medium",
    "sandbox_mode": "workspace-write",
    "warm_session": true,
    "checkpoint_required": true
  }
}
```

Ask-and-continue (WP-AC): Async judgment questions are parked **per Effort** in
`parked_questions` and survive `BeginCycle`. Org runtime status stays runnable
while other Efforts progress. `provide_input` resolves the exact
`interaction_id` without unlocking unrelated parked asks. AC3 execution depth
(per-cycle Effort slate, sequential fan-out, park hand-off) is described under
**WP-AC / AC3 multi-Effort execution** above.

## Cutover & rollback

Full cutover notes from `intern_async_24_7_change_scope.md` (migration &
rollback). Apply before claiming sticky Async + day/month spend bind.

### Migrations (S1b / O5)

Apply Alembic through at least:

- `20260805_intern_async_day_month_budgets` — **S1b** day/month grant columns +
  Async `budget` JSON backfill defaults
- `20260805_intern_grant_expires_at_backfill` — **O5** populate NULL
  `expires_at` on active grants (`created_at + 30d`)
- Follow-ons on the same chain: sticky host leases +
  `20260805_intern_sticky_host_usage_subject` (WP2 S2 idle-burn subject)

### Runtime env

| Variable | Role |
|---|---|
| `INTERN_STICKY_LEASE_STORE` | Default durable Postgres. Set `memory` only for local/unit (process-local; wrong for multi-replica reaper). |
| `INTERN_REQUIRE_MCP_SERVERS` | Defaults on for Intern / `intern-and-smr-runtime`. Set `0` to disable the fail-closed require. |
| `INTERN_EXE_DEV_HOST_CENTS_PER_HOUR` | Sticky exe.dev idle burn rate (default `50` = $0.50/hr) into day/month budgets. |
| `INTERN_ASYNC_EFFORT_FANOUT_MAX` | WP-AC AC3: Efforts one Async cycle may advance (default `3`, clamped to `[1, 16]`). |
| `INTERN_ASYNC_JUDGMENT_STALL_GRACE_SECONDS` | Age after which an all-Efforts-parked assignment pages `intern_async_judgment_stalled` (default `21600`). |
| `INTERN_ACTOR_EXECUTION_JSON` | On **intern-and-smr-runtime** (not the Temporal worker). Staging Async/Sync exe.dev example: |

```json
{
  "sync": {"host_kind": "exe_dev", "warm_session": true},
  "async": {"host_kind": "exe_dev", "warm_session": true, "checkpoint_required": true}
}
```

Async is sticky **exe.dev only**. Sync may use exe_dev (preferred), daytona, or
modal (modal adapter pending).

### Rollback

If Sync/Async/24/7 proof fails after cutover:

1. Redeploy the prior Railway/Temporal worker image and keep the
   `smr-runtime` / `intern-and-smr-runtime` service name as the named rollback
   artifact for the cut window.
2. Pause org Async (frees sticky lease + box); the liveness reaper also frees
   boxes for dead workflows.
3. Do not dual-run forever — restore the prior deployable, then fix forward.
