# Crash recovery: turn ownership and truthful liveness

Workshop used to persist a chat as `running` and render it as **Working** after
the process that owned the turn had died. The sidebar spun, Archive stayed
disabled, and the task could not advance — because its app-server attachment and
in-memory turn owner had gone with the process. Retrying risked duplicating
consequential, paid work.

The system was storing two different facts in one field:

1. **history** — the last known status was `running`;
2. **liveness** — a worker in *this* process owns the turn and can advance it.

Only the first survives a crash. This document is the contract that keeps them
apart.

## The invariant

> A chat may display **Working** only while a live worker owned by the current
> Workshop instance holds a valid claim on its active turn.

Persisted `status = running` alone renders as **Recovering** or
**Interrupted** — never Working.

## Ownership

`instance::boot_epoch()` mints a fresh id on every backend start. It identifies
*this run of the process*, not the installation; a previous owner can therefore
never accidentally match.

`turn_ownership` holds at most one row per session:

| column | meaning |
| --- | --- |
| `session_id` | primary key: one live turn per chat |
| `run_id` | the durable run this claim covers |
| `owner_instance_id` | the boot epoch that took the claim |
| `owner_attachment_id` | the Codex app-server attachment, when there is one |
| `claimed_at` / `heartbeat_at` | when the claim was taken and last refreshed |
| `lease_expires_at` | `heartbeat_at + 20s` |
| `recovery_attempt` | which attempt this turn continues |
| `last_checkpoint_json` | optional progress marker |

`runs` stays an immutable historical record. Liveness is a short-lived,
separately-owned fact, so it gets its own row rather than more mutable columns on
run history.

**Live** means both halves: `owner_instance_id == boot_epoch()` **and** the lease
has not expired. Either half alone is exactly the stale state this exists to
refuse. `recovery::is_live_owner` is the only predicate allowed to decide it.

Heartbeats run at 5s from live provider traffic in the Codex event pump; the
lease is 20s, so four missed heartbeats. That gap is deliberate: XHigh reasoning
can be silent for a long time, and a briefly blocked pump must not interrupt a
healthy turn.

## Reconciliation

`recovery::reconcile_orphaned_turns` rewrites every `running` row whose owner
cannot be proven live. In one transaction it interrupts the run, interrupts the
session, clears `active_run_id`, deletes the claim, and journals
`session/recovery_required`, so no reader can observe a session and its active
run disagreeing.

It is idempotent: a second pass finds nothing, because the first left no
`running` row without a live claim.

**Where it runs matters more than what it does.** It is called inside
`CoreRuntime::open`, synchronously, before the constructor returns — not in a
spawned task. Everything that can read a session goes through a `CoreRuntime`
that already exists, so a task scheduled at startup would race the first read and
let a dead `running` row reach the UI as Working.

The same function backs `CoreRuntime::sweep_expired_leases`, run every 5s by
`spawn_lease_watchdog`. That is what makes liveness independent of an open
renderer window: the renderer's turn watchdogs are cleared when it unloads, so
they cannot fence a turn whose owner died.

### The Codex record cache

`codex/threads.json` — not SQLite — is what the renderer lists Codex chats from
at boot. A database-only reconciliation still left the sidebar showing Working.
`CodexManager::with_paths_and_approvals` therefore corrects that file too, before
`list()` can be called, and copies the durable notice onto each record so the UI
can say what happened rather than just "not running".

## The recovery notice

Written to `sessions.metadata.recovery` and journalled as
`session/recovery_required`, so a client that missed the event still sees it.

```jsonc
{
  "sessionId": "…",
  "runId": "…",
  "reason": "workshop_restarted" | "lease_expired",
  "previousOwnerInstanceId": "inst_…",
  "lastHeartbeatAt": "…",
  "recoveryAttempt": 1,
  "restartable": true,
  "needsAttention": false,
  "externalObjectId": null,
  "lastActivity": { "kind": "item/completed", "label": "container_list", "at": "…" },
  "lastUserMessage": { "text": "…", "clientMessageId": "user-1" },
  "recoveredAt": "…"
}
```

It is cleared when a new turn claims the session, so a recovered chat stops
offering to restart a turn it has already replaced. The replacement run records
`recoveredFromRunId`, `recoveryAttempt` and `recoveredAfterCrash` in its
metadata; the interrupted attempt keeps its own history and is never reopened.

## Side-effect awareness

Replaying a crashed turn is safe only while nothing consequential escaped.
`action_receipts` records what did:

| status | meaning | recovery |
| --- | --- | --- |
| `started` | the request left, the outcome is unknown | `needsAttention`; a human reconciles first |
| `settled` | the external object exists, and its id is recorded | reattach to `externalObjectId`; do **not** launch another |
| `failed` | provably no effect | restartable |
| *(no receipt)* | nothing escaped | restartable |

Ordered by danger, not recency: an unknown settlement outranks a known one,
because the failure it prevents (paying twice) is worse than the one it causes
(asking a human).

Today the wired boundary is the rollout launch
(`POST /v1/containers/{id}/rollouts/start`). The receipt opens *before* the
request and is settled with the rollout id after. A transport error is
deliberately **not** recorded as failed: it does not prove the façade never
accepted the rollout, and claiming it did is how a duplicate gets launched.

## Renderer

`SessionStoreState.liveTurns` holds turns this renderer watched start and has not
watched end. It is deliberately never hydrated: persisted state can say a turn
was running, but only a turn observed starting in this process proves one is.

- granted by `applyTurnAccepted` (the host answered with a real turn id from a
  run it just claimed) and by an unfenced live `run.started`;
- revoked by every terminal run event, `session/unhealthy`,
  `session/recovery_required`, and any local status write that is not `running`;
- pruned, never populated, by `replaceSessions` / `mergeInternSessions`.

`selectWorkingChatIds(sessions, liveTurns)` requires both halves.
`selectChatPresence` maps a session onto:

| presence | when |
| --- | --- |
| `working` | running **and** live |
| `recovering` | running with no live owner |
| `interrupted` | not running, carrying a notice |
| `needsAttention` | notice with `needsAttention` — outranks everything |
| `idle` | otherwise |

`selectChatBusy` locks controls only for `working` / `starting`, so Archive is
never permanently disabled on an ownerless turn. `selectSessionRunning` requires
liveness too: Stop is an instruction to a live worker, and offering it for a turn
nobody owns produces a button that cannot do anything.

`restoreCodexSession` downgrades a `running` record to `interrupted` as a last
line of defence, and preserves the chat's execution target — a recovered
GPT-5.6 Luna chat does not silently become Laguna.

## Fault injection

Recovery is exercised deterministically instead of waiting for an incidental
crash. `SYNTH_DESKTOP_CRASH_AT=<checkpoint>[,<checkpoint>…]` aborts the process
at the named point. Abort, not exit: a graceful shutdown would run the drain path
and hide the failure under test.

| checkpoint | fires |
| --- | --- |
| `after_turn_start` | the durable run exists and has been claimed |
| `after_first_activity` | the first provider event has been journalled |
| `before_tool_dispatch` | an agent tool call is about to cross the IPC boundary |
| `after_tool_dispatch` | that call returned |
| `after_tool_receipt` | a rollout receipt has been settled |
| `after_rollout_launch` | the rollout start request has returned |
| `after_rollout_terminal` | a poll observed the stream closed |
| `before_final_message` | the final agent message is about to be journalled |

`scripts/crash-recovery-drill.sh` drives one checkpoint end to end against a
named instance and reports what the next launch shows.

## What is deliberately not covered

- **The crash trigger itself.** This makes crashes survivable, not rare.
- **Continuing a tool call from its instruction pointer.** Recovery restarts a
  turn; it does not resume one.
- **Automatic replay of unknown-settlement actions.** Those stop at
  `needsAttention` by design.
- **Reopen without the launcher environment.** macOS can reopen the app outside
  `desktop-instance.sh`, losing `SYNTH_DESKTOP_INSTANCE` and with it the
  instance's data root and IPC descriptors. Nothing resumes as Working in that
  state — reconciliation has already run against whichever root was opened — but
  Workshop does not yet *say* it is running degraded. Detecting it needs a
  launcher-side breadcrumb, because the manifest path arrives through the very
  environment that was lost.
