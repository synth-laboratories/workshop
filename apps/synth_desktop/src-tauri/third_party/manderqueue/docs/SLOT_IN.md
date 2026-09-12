# Synth slot-in (P6–P7) — thread-first

Canonical fabric: this repo. **Ontology = thread.** Never publish-to-run.
Run / effort / project are optional **scope labels** for list/filter only.

## Target

```
SMR / Intern / FE / SDK
        │  Synth-minted MQ JWT + mq-sdk / OpenAPI
        ▼
manderqueue (mq-server)  →  Postgres + Redis
        │
        worker → MQ_BRIDGE_BASE_URL  (full message envelope)
```

## Organization workspaces

`org_id` on the credential is the **workspace**. MQ never returns another org’s
threads or messages. List/create ignore client-supplied org overrides — the
token wins. `run_id` is **not** stored in MQ (SMR/Intern bind `run_id → thread_id`).

## Roles (fixed)

| Role | Caps | Typical assignee |
|---|---|---|
| `owner` | read, publish, invite, close | Thread creator |
| `moderator` | read, publish, invite | Sync desk / human co-admin |
| `member` | read, publish | Human collaborators |
| `agent` | read, publish | Intern, SMR actors |
| `observer` | read | Watch-only |

Create/invite take `{ principal, role }` — caps are derived. Fail closed on membership.

## Credentials

| Client | How |
|---|---|
| Human FE/SDK | Org session → Synth mints short-lived MQ JWT (`human`) |
| Intern | At ensure: mint MQ JWT (`intern_async` / `intern_sync`); Intern grant `manderqueue.publish` gates adapter calls; MQ still checks thread role |
| Actor | At register: mint MQ JWT (`actor`) for invited threads |

MQ env: `MQ_AUTH=dev|jwt` (default `dev` = spoofable `Bearer kind:org:id`).  
Prod: `MQ_AUTH=jwt` + `MQ_JWT_SECRET` or JWKS. Claims: `iss`/`aud`=manderqueue, principal in `sub`/`kind`/`org_id`/`id`, `exp`, `jti`. Optional one-shot `thread_bootstrap` for mint-time invite upsert.

## Integration steps

### 1. Deploy

`mq-server serve` + `mq-server worker` with `DATABASE_URL`, `REDIS_URL`, `MQ_AUTH=jwt`, JWT secret/JWKS.

### 2. Intern (Effort judgment thread)

1. On ensure / Effort bind: **ensure** thread with `scope={kind:effort,id:E}` (idempotent list-or-create), participants: human `owner`, Intern `agent`.
2. Publish asks/answers to that `thread_id` (idempotency = operation_id).
3. Observe via per-thread `after_seq` cursor on Intern runtime state (not run-wide SQL).
4. Control plane (ensure/pause/budget) stays off MQ.

### 3. SMR (swarm correlation thread)

1. On run start: **ensure** thread via `POST /v1/threads/ensure` with `idempotency_key=smr:run:{R}` (and a soft label like `scope={kind:project,id:…}` — **never** `scope=run`); persist `run_id→thread_id` in SMR. Invite actors/orchestrator as `agent`, Intern as `agent`, operators as `member`/`moderator`.
2. Publish steers with optional `recipients[]` for directed delivery; use `parent_message_id` / `causation_id` for Intern correlation.
3. SMR stores `run_id → thread_id` in **SMR state**; all publish/steer use `thread_id`.
4. Deprecate `audience_kind=run`; shim old callers through the map once.
5. Worker POSTs full envelope to bridge (message body/kind/payload/sender + job).

### 4. Cutover

Dual-read if needed → stop Python MQ writes → delete `backend/packages/manderqueue` + `services/manderqueue` after green.

## Bridge envelope (`POST {MQ_BRIDGE_BASE_URL}/v1/delivery`)

```json
{
  "job_id": "...",
  "message_id": "...",
  "thread_id": "...",
  "recipient": { "kind": "actor", "id": "...", "org_id": "..." },
  "attempts": 1,
  "message": {
    "seq": 3,
    "kind": "steer",
    "body": "...",
    "payload": {},
    "sender": { "kind": "intern_async", "id": "...", "org_id": "..." },
    "idempotency_key": null,
    "correlation_id": null,
    "created_at": "..."
  }
}
```

## Non-goals of this repo

- FE Messages UI, public MCP `research_mq_*` (P8)
- Project-event Redis fan-out (SMR owns)
- Temporal / ensure / pause
- Org-custom role registries
