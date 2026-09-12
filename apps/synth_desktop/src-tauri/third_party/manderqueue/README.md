# Manderqueue

**Private** · [synth-laboratories/manderqueue](https://github.com/synth-laboratories/manderqueue)

Permissioned messaging fabric: **threads · roles · publish/tail · delivery jobs · Redis write-buffer + wake · SDK**.

> **Status: v0.1+** (thread-first roles, JWT auth, Redis→batch PG, Intern/SMR adapter stubs)  
> Synth product slot-in: [`docs/SLOT_IN.md`](docs/SLOT_IN.md)  
> Plan: [`plans/PLAN.md`](plans/PLAN.md)

## Crates

| Crate | Role |
|---|---|
| `mq-core` | Domain, `Store` port, memory store, `BatchingStore`, wake traits |
| `mq-server` | Axum API, Postgres, Redis write buffer + wake, SSE, worker |
| `mq-sdk` | Typed HTTP client |

## Run

```bash
docker compose up -d postgres redis

export DATABASE_URL=postgres://mq:mq@127.0.0.1:5433/manderqueue
export REDIS_URL=redis://127.0.0.1:6380
# Optional: buffer hot publishes then batch-flush to Postgres (reduces PG load)
# export MQ_WRITE_BUFFER=memory   # or redis | off (default)
# export MQ_WRITE_BATCH_SIZE=50
# export MQ_WRITE_FLUSH_MS=25
# Auth: MQ_AUTH=dev (default spoof Bearer kind:org:id) | jwt + MQ_JWT_SECRET

cargo run -p mq-server -- serve
cargo run -p mq-server -- worker   # MQ_BRIDGE_BASE_URL optional; POSTs full message envelope

# OpenAPI
curl -s localhost:8088/openapi.yaml | head
```

Dev auth: `Authorization: Bearer {kind}:{org_id}:{id}`  
(`human` \| `intern_async` \| `intern_sync` \| `actor` \| `system`)

## Tests

```bash
cargo test --workspace

DATABASE_URL=postgres://mq:mq@127.0.0.1:5433/manderqueue \
  cargo test -p mq-server --test postgres_fabric -- --ignored

# Measure PG write load: sync (O(N) txns) vs batch / Redis→PG (1 txn for N publishes)
DATABASE_URL=postgres://mq:mq@127.0.0.1:5433/manderqueue \
REDIS_URL=redis://127.0.0.1:6380 \
  cargo test -p mq-server --test batch_pg_load -- --ignored --nocapture --test-threads=1
```

## API (v1)

| Method | Path |
|---|---|
| GET | `/health` `/ready` `/openapi.yaml` |
| POST/GET | `/v1/threads` |
| POST | `/v1/threads/ensure` (idempotent create; requires `idempotency_key`) |
| GET | `/v1/threads/{id}` |
| POST | `/v1/threads/{id}/participants` |
| POST/GET | `/v1/threads/{id}/messages` (`recipients[]`, `parent_message_id`, `causation_id`) |
| GET | `/v1/threads/{id}/events` (SSE) |

Worker delivers to `POST {MQ_BRIDGE_BASE_URL}/v1/delivery` when set; otherwise stub-settles.

Hard law: **transport only** — not Intern/SMR ensure, pause, budget, or Temporal.

## Embedded checkpoint mode

`mq-server embedded` runs a container-local memory fabric with boot-time restore
(`MQ_RESTORE_FILE`) and a control-token-protected whole-instance checkpoint
(`POST /_embedded/checkpoint`, `MQ_CHECKPOINT_TOKEN`, at least 32 characters).
It binds loopback only and rejects external Postgres/Redis/bridge configuration
and write buffering. No external delivery worker belongs in this mode.

Restore starts a new isolated store and preserves thread/message IDs, sequence
numbers, participants, idempotency and jobs. It never rewinds a shared production
service. The controller must checkpoint its own logical inboxes/cursors and the
environment alongside MQ; external effects and ephemeral wake subscribers are
not restored. This endpoint is absent from normal `serve` mode.

MAPO's combined container and cold-restore contract are documented in
[`evals/mapo/container/README.md`](../evals/mapo/container/README.md).

## Rust client recovery

Scoped HTTP thread/message reads validate persisted membership, read permission
and grant generation in the same store critical section as their returned
snapshot. Memory uses one mutex; Postgres holds thread and participant share
locks until the bounded read commits. SSE uses this path for its authorization
rechecks. A read that linearized before revocation may still finish sending its
response; data already read cannot be recalled. Token expiry is checked at HTTP
authorization and stream rechecks, not a new database-stored expiry contract.
The legacy volatile buffer is not merged into scoped reads. Device issuance,
history grants and native execution fencing remain separate integration work.

`mq_sdk::CatchUpSupervisor` restores an account/thread-scoped durable message
cursor and reads pages of at most 200 messages. Call `catch_up` on connection,
`thread_wake`, `resync`, and periodic polling. SSE notifications are hints; never
store an event hint as the accepted inbox cursor. Each call has a page budget
of 1–100 and returns `PageBudgetReached` when another pass may be needed.

Pages must contain contiguous sequence numbers and unique message identities;
foreign-thread, skipped, reordered, duplicate-identity and oversized pages are
refused before persistence. This uses the complete-thread history endpoint;
future filtered grants require an explicit server paging cursor contract.

The supplied commit callback must atomically store the page and next cursor in
the local inbox before returning success. Deduplicate by message ID: failed or
cancelled commits can replay. The supervisor advances its in-memory cursor only
after success. Restore from the committed cursor after restart, and discard the
supervisor on account changes. HTTP authorization failures propagate to the
caller, which must stop until authority is restored. JSON client requests have a
30-second deadline and never follow redirects, including redirects within the
same origin. Redirect responses remain API failures at the configured endpoint.
Successful JSON bodies are bounded to 16 MiB and error bodies to 64 KiB during
streaming, whether or not Content-Length is present. Oversized successful pages
fail without advancing a catch-up cursor; oversized errors retain HTTP status
with a fixed diagnostic instead of retaining the response body.
This helper does not execute messages, connect an SSE stream,
implement fine-grained grants, or supply Workshop's inbox database.
