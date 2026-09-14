# Manderqueue global messaging fabric

**Date:** 2026-08-05  
**Status:** **v0.1+ in this repo.** Postgres SoT, Redis write-buffer → batch PG + wake, HTTP+SSE,
worker (+ optional bridge webhook), OpenAPI, `mq-sdk`.  
**Remaining outside this repo:** Synth slot-in + Python MQ deprecation — see `docs/SLOT_IN.md`.

**Companions (current Python MQ — to be replaced):**
- `backend/packages/manderqueue/` + `packages/manderqueue/HANDOFF.md`
- `backend/services/manderqueue/README.md`
- `backend/plans/smr/intern_manderqueue_handoff.md` (Intern → run actors; transport only)
- `backend/specifications/tanha/current/systems/intern/runtime_authority.md` (MQ ≠ Intern control plane)
- Shape reference (ideas only): [Block Buzz](https://github.com/block/buzz) — humans + agents as equals on a shared room fabric; **not** a Nostr/relay adoption plan

---

## Goal

Formalize Manderqueue as Synth’s **shared, permissioned messaging service** — a **feature
superset** of today’s run-scoped actor bus — that **everything** uses:

| Participant | Uses MQ for |
|---|---|
| Humans / operators | Steer, ask/answer, cross-boundary talk |
| Sync Intern | Desk ↔ human, Sync ↔ Async, Sync ↔ run actors |
| Async Intern | Judgment asks, Effort-thread talk, actor steer |
| Factories / Efforts | Program-level threads (not only run chat) |
| SMR runs / swarms / actors | Existing delivery path (slot-in, not fork) |

Users should be able to **message across Intern / Factory / SMR / actor boundaries** under
explicit permissions, without a second Slack product and without making MQ the control plane.

```
                    ┌─────────────────────────────────────┐
                    │   Permissioned MQ service (target)  │
                    │   Threads · participants · publish  │
                    │   fan-out · ingress · audit         │
                    └───────────────┬─────────────────────┘
                                    │
         ┌──────────────┬───────────┼───────────┬──────────────┐
         ▼              ▼           ▼           ▼              ▼
   Sync Intern    Async Intern   Factory    SMR run/swarm   Humans
   (Temporal +     (Temporal +   / Effort    actors         (FE/SDK)
    PG mailbox)     PG mailbox)
         │              │
         └── control plane stays here (ensure / pause / budget / lease)
```

**Hard law (unchanged):** MQ is **transport only**. Ensure, pause, budgets, sticky lease,
generation CAS, Temporal wake stay on Intern/SMR control planes + PG mailboxes.

---

## Design patterns

Keep the codebase boring and hexagonal. Prefer the patterns already proven in Python MQ;
do not invent a new paradigm.

| Pattern | How we use it |
|---|---|
| **Hexagonal / ports & adapters** | `mq-core` = pure domain (threads, membership, publish/read rules). Adapters: HTTP/OpenAPI, Postgres store, Redis wakeup, delivery worker, runtime bridge client. |
| **Capability + identity authZ** | Every request carries a **principal** and **credential**. Authorize on thread **membership + role → caps** (`read` / `publish` / `invite` / `close`). Fail closed. |
| **Idempotent commands** | Publish (and create-thread where needed) take `idempotency_key`; retries return the same receipt. |
| **Append-only message log** | Thread history is an ordered append log + cursors. **Not** full event sourcing / CQRS theater. |
| **Transactional outbox → delivery jobs** | Hot-path writes land in a **Redis write buffer**, then a flusher **batch-inserts** messages + jobs into Postgres in one txn. Worker claims jobs from PG; at-least-once; consumers dedupe. |
| **Soft fan-out accelerator** | Redis pubsub also **wakes** workers/SSE. Wake loss is OK; **write-buffer loss is not** — buffer must be durable Redis (AOF/stream) or fail closed to sync PG. |
| **PG load reduction** | Prefer Redis → batch PG over per-publish PG round-trips. **Measure** with integration tests (txn/statement counts under N publishes). |
| **Adapter / anti-corruption for SMR** | Legacy `audience=run` shims to `run_id → thread_id` in **SMR state**, then thread APIs. Python MQ deprecated; no dual bus long-term. |
| **Control-plane firewall** | No ensure / pause / budget / Temporal wake APIs on MQ. Soft “notify participant” only. |
| **Typed errors + newtype IDs** | Rust: `ThreadId`, `MessageId`, `Principal`, `Role`, domain `Error` codes stable in OpenAPI. |

**Explicitly avoid:** chat-as-orchestrator, Nostr/relay federation, global unscoped rooms, making Redis the source of truth, embedding Intern Temporal inside MQ, **using run as the messaging ontology**.

### Crate layout (intended)

```
crates/mq-core     domain + ports (traits) + in-memory fake for tests
crates/mq-server   axum HTTP, OpenAPI, wiring, delivery worker loop
crates/mq-sdk      typed HTTP client (generated or hand-maintained from OpenAPI)
```

TDD: unique fabric invariants in `mq-core`; HTTP e2e in `mq-server` (and later sdk e2e).

---

## Infrastructure

| Piece | Role | Required? |
|---|---|---|
| **Postgres** | Durable source of truth after flush: threads, membership, messages, delivery jobs | **Yes** |
| **Redis** | (1) **Write buffer** for hot publish path → batch flush to PG (2) wakeup/pubsub for SSE/worker | **Yes for prod**; degrade to sync PG writes if Redis buffer unavailable |
| **HTTP service (Rust)** | Permissioned OpenAPI surface; health/ready | **Yes** |
| **Delivery worker** | Claims PG delivery jobs → runtime bridge / ingress hooks (can be same binary, separate process/command) | **Yes** |
| **Object storage** | No — attachments are **refs** to existing SMR media/visuals | No |
| **Temporal / Kafka / NATS** | No inside MQ — control plane and heavy streaming stay elsewhere | No |
| **Auth issuer** | Trust Synth-issued tokens / API keys (org user broad; agent narrow). MQ validates; does not become IdP | External |

```
                    ┌─────────────┐
   clients ────────►│ mq-server   │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────────────┐
              ▼            ▼                    ▼
        ┌──────────┐ ┌───────────┐        ┌──────────┐
        │ Postgres │ │  Redis    │        │  Redis   │
        │ (SoT     │ │  WRITE    │───────►│  WAKE    │
        │  after   │ │  BUFFER   │ batch  │  pubsub  │
        │  flush)  │ │  stream   │ flush  └──────────┘
        └────┬─────┘ └───────────┘
             │ claim jobs
             ▼
        delivery worker
```

**Write path (hot):** authz → append to Redis stream `mq:write` → ack client → flusher XREAD → **one PG txn** for N messages+jobs.  
**Read path:** PG (durable) ∪ unflushed buffer (read-through) so clients see their writes immediately.  
**Degraded:** if Redis buffer down → sync write to PG (higher load, still correct).

### Deploy targets

| Target | Infra |
|---|---|
| **synth-dev local compose** | `manderqueue` service + shared or dedicated **Postgres** + **Redis** (reuse stack Redis/PG where possible) |
| **Railway** | Same binary; env for `DATABASE_URL`, `REDIS_URL`, auth trust keys |
| **Staging / prod** | Same; worker as second process or sidecar command on same image |

### Local TDD without full infra

- **Unique domain tests:** in-memory store in `mq-core` (no Docker).  
- **HTTP e2e:** in-memory or ephemeral PG (later `testcontainers` / compose profile).  
- First green bar: in-memory + axum; Postgres adapter next; Redis wakeup last.

---

## Full implementation plan

**Infra locked:** **Postgres (durable SoT) + Redis (write buffer + wake).** No Kafka/NATS/Temporal inside MQ.

### Redis → batch Postgres (PG load)

```
  publish (N times)
       │
       ▼
  Redis stream mq:write   ◄── durable buffer (XADD)
       │
       │  flusher every batch_size OR interval
       ▼
  BEGIN;
    INSERT mq_messages multi-row
    INSERT mq_delivery_jobs multi-row
  COMMIT;                 ◄── 1 txn ≈ batch_size publishes
       │
       ▼
  XACK / trim stream
```

| Knob | Default (sketch) |
|---|---|
| `MQ_WRITE_BATCH_SIZE` | 50 |
| `MQ_WRITE_FLUSH_MS` | 25 |
| `MQ_WRITE_BUFFER` | `redis` \| `memory` \| `off` (sync PG) |

**Integration tests must measure:** under N publishes, PG transaction count (and/or statement count) with buffering **≪** N; with `off`, ≈ N. See `crates/mq-server/tests/batch_pg_load.rs`.

### How it works (system)

```
                         ┌──────────────────────────────────────────────────┐
                         │              CLIENTS                              │
                         │  FE · SDK · public MCP · Intern · SMR adapters    │
                         └───────────────────────┬──────────────────────────┘
                                                 │ HTTPS + bearer / API key
                                                 │ OpenAPI: threads / publish / tail
                                                 ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                         mq-server  (Rust binary)                            │
│  ┌─────────────┐   ┌──────────────┐   ┌─────────────┐   ┌───────────────┐ │
│  │ AuthN/Z     │──►│ Domain       │──►│ Store port  │──►│ Wake port     │ │
│  │ principal + │   │ (mq-core)    │   │ (Postgres)  │   │ (Redis)       │ │
│  │ membership  │   │ threads msg  │   │             │   │ optional      │ │
│  └─────────────┘   └──────────────┘   └──────┬──────┘   └───────┬───────┘ │
│                                              │                  │         │
│  ┌───────────────────────────────────────────┴──────────────────┘         │
│  │  same binary, `mq-server worker` command                               │
│  │  claim delivery_jobs → deliver to actor bridge / intern webhook        │
│  └────────────────────────────────────────────────────────────────────────┘
└────────────────────────────────────────────────────────────────────────────┘
           │                                              │
           ▼                                              ▼
   ┌───────────────┐                              ┌───────────────┐
   │   Postgres    │                              │     Redis     │
   │ threads       │                              │ channel wake  │
   │ participants  │                              │ sse notify    │
   │ messages      │                              │ (lossy OK)    │
   │ delivery_jobs │                              └───────────────┘
   │ idempotency   │
   └───────────────┘
```

### Sequence — Effort judgment (ask → answer → continue)

```
 Human FE/SDK          Async Intern           mq-server            Postgres         Redis
     │                      │                     │                   │               │
     │                      │ POST /threads       │                   │               │
     │                      │  scope=effort E     │                   │               │
     │                      │  members=[async,    │                   │               │
     │                      │           human]    │                   │               │
     │                      │────────────────────►│ INSERT thread+    │               │
     │                      │                     │ participants      │               │
     │                      │                     │──────────────────►│               │
     │                      │                     │ PUBLISH wake      │               │
     │                      │                     │──────────────────────────────────►│
     │                      │                     │◄──────────────────│               │
     │                      │ 201 thread_id=T     │                   │               │
     │                      │◄────────────────────│                   │               │
     │                      │                     │                   │               │
     │                      │ POST /messages      │                   │               │
     │                      │  kind=ask body=…    │                   │               │
     │                      │  idempotency_key=k1 │                   │               │
     │                      │────────────────────►│ txn: message +    │               │
     │                      │                     │ jobs for human    │               │
     │                      │                     │──────────────────►│               │
     │                      │                     │ wake human/FE     │               │
     │                      │                     │──────────────────────────────────►│
     │  SSE/tail notify     │                     │                   │               │
     │◄───────────────────────────────────────────│◄──────────────────│◄──────────────│
     │ GET /threads/T/messages                    │                   │               │
     │───────────────────────────────────────────►│ SELECT after      │               │
     │◄───────────────────────────────────────────│ cursor            │               │
     │                      │                     │                   │               │
     │ POST kind=answer     │                     │                   │               │
     │  correlation=ask_id  │                     │                   │               │
     │───────────────────────────────────────────►│ txn: message +    │               │
     │                      │                     │ job for async     │               │
     │                      │                     │──────────────────►│               │
     │                      │                     │ wake              │               │
     │                      │                     │──────────────────────────────────►│
     │                      │  worker/ingress     │                   │               │
     │                      │  observe answer     │                   │               │
     │                      │◄────────────────────│ (pull or push)    │               │
     │                      │  WP-AC unpark       │                   │               │
     │                      │  (Intern plane)     │                   │               │
```

### Sequence — Run steer (human/Intern → actors)

```
 Publisher              mq-server                 Postgres              Worker           Actor runtime
     │                      │                        │                    │                   │
     │ POST publish         │                        │                    │                   │
     │ thread=run-auto(R)   │                        │                    │                   │
     │ kind=steer           │                        │                    │                   │
     │─────────────────────►│ INSERT message         │                    │                   │
     │                      │ INSERT jobs per actor  │                    │                   │
     │                      │ member on thread       │                    │                   │
     │                      │───────────────────────►│                    │                   │
     │                      │ Redis wake workers     │                    │                   │
     │                      │────────────────────────────────────────────►│ (optional wake)   │
     │                      │                        │  CLAIM job         │                   │
     │                      │                        │◄───────────────────│                   │
     │                      │                        │                    │ deliver            │
     │                      │                        │                    │──────────────────►│
     │                      │                        │  SETTLE / DLQ      │                   │
     │                      │                        │◄───────────────────│                   │
```

### Data model (Postgres)

```
┌──────────────────┐       ┌─────────────────────────┐
│ mq_threads       │       │ mq_participants         │
│──────────────────│       │─────────────────────────│
│ thread_id  PK    │◄──────│ thread_id  FK           │
│ org_id           │       │ principal_kind          │  human|intern_sync|
│ scope_kind       │       │ principal_id            │  intern_async|actor|system
│ scope_id         │       │ caps  (read,publish,…)  │
│ title / kind     │       │ UNIQUE(thread,principal)│
│ created_at       │       └─────────────────────────┘
└────────┬─────────┘
         │
         ▼
┌──────────────────┐       ┌─────────────────────────┐
│ mq_messages      │       │ mq_delivery_jobs        │
│──────────────────│       │─────────────────────────│
│ message_id PK    │◄──────│ message_id FK           │
│ thread_id FK     │       │ recipient_principal     │
│ seq (per thread) │       │ status pending|…|dlq    │
│ kind ask|answer| │       │ attempts / next_at      │
│   steer|notice…  │       │ UNIQUE(message,recip) │
│ body / payload   │       └─────────────────────────┘
│ sender_principal │
│ idempotency_key  │── UNIQUE(org, key) when set
│ correlation_id   │
│ created_at       │
└──────────────────┘
```

Redis keys (ephemeral): `mq:wake:thread:{id}`, `mq:wake:worker`, optional presence later.

### Component UML (logical)

```
┌────────────┐     uses      ┌────────────┐
│  mq-sdk    │──────────────►│ OpenAPI    │
└────────────┘               │ HTTP API   │
                             └─────┬──────┘
                                   │ implements
                             ┌─────▼──────┐
                             │ mq-server  │
                             │ (axum)     │
                             └─────┬──────┘
                    ┌──────────────┼──────────────┐
                    │              │              │
              ┌─────▼─────┐ ┌──────▼─────┐ ┌─────▼──────┐
              │ mq-core   │ │ Postgres   │ │ RedisWake  │
              │ domain    │ │ Store      │ │ adapter    │
              └───────────┘ └────────────┘ └────────────┘
                    ▲
                    │ worker loop also uses domain + store
              ┌─────┴──────────┐
              │ DeliveryWorker │──► RuntimeBridge / InternIngress HTTP
              └────────────────┘
```

### Phased build (implementation order)

| Phase | Deliverable | Tests | Infra |
|---|---|---|---|
| **P0 — Domain TDD** | `mq-core`: thread/member/publish/tail, fail-closed, idempotent, Effort thread w/o run | Unique fabric unit tests (in-memory) | none |
| **P1 — HTTP skeleton** | `mq-server` axum + OpenAPI stub; health; bearer principal | HTTP e2e vs in-memory | none |
| **P2 — Postgres** | Migrations + `PostgresStore`; jobs in same txn as publish | e2e + integration w/ PG (compose/testcontainers) | **Postgres** |
| **P3 — Worker** | `mq-server worker`; claim/settle/DLQ; stub bridge | worker integration | Postgres |
| **P4 — Redis wake** | Publish → Redis notify → worker/SSE faster path; degrade if Redis down | chaos: kill Redis, messages still delivered via poll | **Postgres + Redis** |
| **P5 — SDK** | `mq-sdk` (Rust) + OpenAPI artifact; optional TS/Python later | SDK e2e against server | PG+Redis |
| **P6 — Synth slot-in** | SMR adapter (run auto-thread); Intern adapter; compose + Railway | contract tests vs staging | full |
| **P7 — Deprecate Python** | Dual-read then cut; remove `backend` MQ paths | migration checklist | full |
| **P8 — Product surfaces** | public MCP tools; FE Messages; judgment queue | Playwright / MCP contract | full |

Map to earlier WP labels: P0–P1 ≈ MQ0; P2–P3 ≈ MQ1; P4–P5 ≈ MQ0/4; P6 ≈ MQ1–2; judgment Effort ≈ MQ3; audit/limits ≈ MQ5.

### API surface (v1 ship)

| Method | Path | Notes |
|---|---|---|
| `GET` | `/health` `/ready` | ready = PG ping (Redis optional) |
| `POST` | `/v1/threads` | create + initial participants |
| `GET` | `/v1/threads` | list by scope; membership filter |
| `GET` | `/v1/threads/{id}` | |
| `POST` | `/v1/threads/{id}/participants` | invite |
| `POST` | `/v1/threads/{id}/messages` | publish; idempotency key |
| `GET` | `/v1/threads/{id}/messages` | cursor/tail |
| `GET` | `/v1/threads/{id}/events` | SSE (Redis wake + PG reread) |
| `POST` | `/v1/delivery/ack` | optional explicit ack from bridges |

No `/ensure`, `/pause`, `/budget` — control-plane firewall.

### Auth

```
Authorization: Bearer <token>
  → validate (Synth API key or scoped agent JWT)
  → Principal { kind, id, org_id, caps_default }
  → each op: membership ∩ required cap  OR  deny 403
```

Users: broad org defaults. Intern/actors: narrow scoped tokens minted by Synth admission.

### Cutover from Python MQ

```
Phase A   Rust MQ green in compose (Intern still on Python)
Phase B   SMR publish path → Rust (adapter); dual-read history if needed
Phase C   Intern adapter → Rust; Python write path off
Phase D   Delete backend packages/services manderqueue (+ worker move)
```

**No long dual-write.** Prefer short dual-read window then hard cut (same philosophy as Intern cutover).

### Testing strategy (TDD)

| Layer | What |
|---|---|
| **Unique** | Effort w/o run; non-member 403; idempotent replay; human+async+actor same thread; no control-plane routes |
| **E2E** | HTTP judgment flow; run-steer → job → stub bridge; Redis down still delivers |
| **SDK** | Same flows via `mq-sdk` |
| **Soak** | Job backlog, cursor correctness, DLQ |

### Env (sketch)

```
DATABASE_URL=postgres://…
REDIS_URL=redis://…          # optional; empty = poll-only
MQ_BIND=0.0.0.0:8088
MQ_AUTH_MODE=synth_api_key   # or jwt
MQ_BRIDGE_BASE_URL=…         # actor/intern ingress
```

### Compose / Railway

- **compose:** service `manderqueue` (api) + same image `command: worker`; share stack `postgres` + `redis`.
- **Railway:** one service api, one worker (or one dyno two processes); secrets for DB/Redis/auth.

---

## Why after Intern (not during)

Intern cut already depends on MQ for actor transport and plans thin H6/H7 human/cross-lane
ingress. Expanding MQ into a global fabric during the same cut risks:

- dual buses (old run MQ + new “global MQ”)
- permission model unfinished while Async ask-and-continue (WP-AC) is still landing
- SMR delivery regressions mid–service rename (`intern-and-smr-runtime`)

**Ratchet:** Intern push keeps using **current** MQ APIs. This doc is the **follow-on
formalization**.

---

## Today → target

| | **Today** | **Target (superset)** |
|---|---|---|
| Scope | Mostly **run-bound** subscribers + audience | **Org messaging plane** with Thread as first-class conversation scope |
| Participants | Actors (+ some human publish paths) | Human, Sync, Async, Factory ops, swarm actors — first-class equals on the fabric |
| Intern bind | Often requires `project_id` + `run_id` to publish | Policy may allow Effort/org threads **without** a live run (judgment asks) |
| Client↔Intern ensure/send | **Not** MQ (PG mailbox) | **Still not** MQ |
| SMR path | `services.manderqueue.application` + delivery worker | Same worker/store behind a **stable public MQ API**; SMR is a client |
| UX | Run/actor chrome | Cross-boundary threads visible on Effort board / Sync desk / Factory |

---

## Product shape (what users feel)

**Thread** = durable conversation instance with:

- `thread_id`
- **scope binding** (one primary, optional links): `org` | `factory` | `effort` | `project` | `run` | `sync_session` | `async_runtime`
- **participants** (subscriber records): `human`, `intern_sync`, `intern_async`, `actor`, `system`, …
- **membership / capabilities** (who may read, publish, invite, close)
- message kinds: steer, ask, answer, notice, actor_runtime, …
- idempotent publish + at-least-once delivery; consumers dedupe

Cross-boundary example (Craftax hobbyist):

```
Effort "Craftax PPO baseline"
  └── Thread t1  (judgment)
        ├── Async Intern: "Which primary metric?"
        ├── Human: "dense + success"
        └── (optional) Sync Intern joined for live dig — same thread_id
```

Same fabric, different control-plane ingress when each side needs to **act**.

---

## Permissioned API (sketch)

Stable surface (HTTP + SDK + public MCP; agent grants separate):

| Verb | Intent |
|---|---|
| `mq.thread.create` | Open thread under a scope binding + initial participants |
| `mq.thread.get` / `list` | Scoped list; membership-filtered |
| `mq.thread.add_participant` | Invite under policy |
| `mq.publish` | Idempotent message into thread (or legacy run audience) |
| `mq.read` / `tail` / `cursor` | Bounded pull; stable cursors |
| `mq.ack` / delivery receipts | As today, refined |

**AuthZ rules (fail closed):**

1. Principal must be in org.
2. Scope binding must resolve (Effort/Run/… exists and principal may see it).
3. Publish/read requires thread membership **or** explicit capability grant.
4. Fan-out only to members (Buzz lesson: no leaking private channel events to “global” subs).
5. Cross-boundary does **not** imply cross-control-plane authority (messaging ≠ can pause Async).

Legacy run APIs remain as **adapters** calling the same service (`AudienceSelection` → thread
or ephemeral run-thread).

---

## Deployment targets (when implemented)

| Target | Expectation |
|---|---|
| **Local** | Runs via `synth-dev` local compose (service + Postgres; Redis if fan-out needs it) |
| **Railway** | Same image/config pattern as other Synth services |
| **Staging / prod** | Permissioned HTTP + MCP; SMR/Intern are clients |

Exact compose service name / env vars — TBD at implementation time (not in this init).

---

## Slot-in for SMR (non-negotiable)

```
SMR / swarm / actor code today
        │
        ▼
services.manderqueue.application  (keep working)
        │
        ▼
[this refactor]  thin facade / version bump
        │
        ▼
same PG message + delivery jobs + worker + runtime bridge
```

- **No second delivery worker.**
- **No dual-write long term** — migrate addressing to Thread; keep run-audience as a view or
  auto-thread per run.
- Intern `manderqueue_adapter` becomes a client of the same public API (capability-gated).

---

## Work packages (summary)

Detailed phases, sequences, schema, and cutover live in **Full implementation plan** above
(P0–P8). Short map:

| WP | Outcome |
|---|---|
| **MQ0** | OpenAPI + HTTP; domain TDD green (P0–P1) |
| **MQ1** | Postgres threads/membership/jobs; run auto-thread (P2–P3, P6 start) |
| **MQ2** | Human + intern_sync + intern_async participants + ingress (P6) |
| **MQ3** | Effort/Factory threads without live run (P0 invariant → P6) |
| **MQ4** | SDK + public MCP + FE Messages (P5, P8) |
| **MQ5** | Audit, retention, rate limits, cross-org deny (P8 / soak) |

**Out of scope:** replacing Intern PG mailbox; MQ as Temporal wake; Buzz/Nostr; Slack clone.

---

## Relationship to Intern 24/7 scope

| Intern doc | This doc |
|---|---|
| MQ = messaging fabric; not control plane | Same law |
| H6/H7 Sync↔Async↔human on MQ | Minimal path during Intern; **full** Thread model here |
| WP-AC per-Effort ask-and-continue | Judgment **state** in Intern reducer; judgment **messages** may ride MQ threads (post-Intern polish) |
| Downstream FE/SDK/MCP | Intern cut ships Effort board; MQ4 adds rich cross-boundary messaging UX |

Do **not** block Intern Buildout 0–2 on MQ0–MQ5.

---

## Organization workspaces (hard tenancy)

**Every thread, participant, message, and delivery job belongs to exactly one
`org_id` (workspace).** Two orgs never share threads or see each other’s
messages — fail closed.

| Rule | Enforcement |
|---|---|
| Credential binds `org_id` | JWT / dev bearer always carries org; no org-less principals |
| Client cannot pick another org | List/create use **token** `org_id` only (query `org_id` ignored/forbidden) |
| Participants must match thread org | Reject invite if `principal.org_id != thread.org_id` |
| Messages stamped with thread org | `mq_messages.org_id` = thread’s org; idempotency unique per org |
| Cross-org UUID probe | Same as non-member: **404/403**, never leak other-org content |
| `run_id` | **Never stored in MQ** — SMR/Intern hold `run_id → thread_id` |

Workspace ≠ membership. Within an org, threads are still permissioned by role/caps.
Across orgs, there is no shared fabric at all.


### Fixed roles → caps

| Role | Caps | Typical assignee |
|---|---|---|
| `owner` | read, publish, invite, close | Thread creator |
| `moderator` | read, publish, invite | Sync desk / human co-admin |
| `member` | read, publish | Human collaborators |
| `agent` | read, publish | Intern, SMR actors |
| `observer` | read | Watch-only |

Create/invite take `{ principal, role }`; caps are derived and stored for enforcement.
Fail closed: no membership row → deny.

### Credentials (Synth IdP → MQ validates)

| Client | Credential |
|---|---|
| Human FE/SDK/MCP | Org session → Synth mints short-lived MQ JWT as `human` |
| Intern | At ensure: MQ JWT as `intern_*`; Intern grant `manderqueue.publish` gates adapter; MQ checks thread role |
| Actor | At register: MQ JWT as `actor` for invited threads |

`MQ_AUTH=dev` (local spoof bearer) \| `jwt` (prod). Claims: `iss`/`aud`=manderqueue, principal, `exp`, `jti`; optional one-shot `thread_bootstrap`.

---

## How Intern, SMR actors, and users plug in

```
                         ┌──────────────────────────────────┐
                         │     Permissioned MQ service      │
                         │  threads · roles · publish/tail  │
                         └────────────────┬─────────────────┘
                                          │
           ┌──────────────────────────────┼──────────────────────────────┐
           │                              │                              │
           ▼                              ▼                              ▼
    ┌──────────────┐              ┌──────────────┐              ┌──────────────┐
    │ HUMAN USER   │              │ INTERN       │              │ SMR ACTORS   │
    │ owner/member │              │ agent on     │              │ agent on     │
    │ + MQ JWT     │              │ Effort/swarm │              │ swarm thread │
    └──────┬───────┘              │ threads      │              └──────┬───────┘
           │                      └──────┬───────┘                     │
     FE / SDK                     ensure→invite→publish         delivery worker
     (user MQ JWT)                (intern MQ JWT)               → bridge envelope
```

Control planes **create/ensure threads and invite principals**. They do not address runs as rooms.

### Intern plug-in

1. On ensure / Effort bind: ensure Effort judgment thread (`scope=effort:E`), human `owner`, Intern `agent`.
2. Publish to that `thread_id` (idempotency = operation_id).
3. Observe via per-thread `after_seq` cursor on Intern runtime state (not run-wide SQL).
4. Ensure/pause/budget stay off MQ.

### SMR plug-in

1. On run start: ensure swarm thread labeled `scope=run:R` (**correlation only**); invite actors/orchestrator `agent`, Intern `agent`, operators `member`/`moderator`.
2. SMR stores `run_id → thread_id` in **SMR state**; all publish uses `thread_id`.
3. Worker POSTs full message envelope to `MQ_BRIDGE_BASE_URL`.
4. Shim legacy `audience=run` → resolve map → thread API.

### End-to-end (one Effort)

```
User FE                    Async Intern                 Swarm actors
   │                            │                            │
   │  ensure Async on E         │                            │
   │                            │  MQ: ensure Effort thread  │
   │                            │  kickoff swarm via SMR MCP │
   │                            │───────────────────────────►│
   │                            │  SMR: ensure swarm thread  │
   │                            │  (label scope=run:R)       │
   │  MQ: answer on Effort T    │  MQ: ask on Effort T       │
   │◄──────────────────────────►│                            │
   │                            │  MQ: steer on swarm T'     │
   │                            │───────────────────────────►│
```

See `docs/SLOT_IN.md` for deploy, token mint, and bridge JSON.

---

## Potential features (brainstorm — not committed)

Prioritize later; **P0** = fabric won’t work without; **P1** = makes Intern/SMR feel one product;
**P2** = Buzz-adjacent / nice; **P3** = maybe never.

### P0 — Core fabric

| Feature | Why |
|---|---|
| **Threads** as durable conversation instances | Unit users and agents address |
| **Scope binding** (org / factory / effort / project / run / sync / async) | Cross-boundary without a free-for-all |
| **Participants** as first-class equals (human, sync, async, actor, system) | Same fabric everywhere |
| **Membership + capabilities** (read / publish / invite / close) | Permissioned APIs |
| **Idempotent publish + durable delivery + cursor read/tail** | Superset of today’s job worker |
| **Fail-closed fan-out** | No leaking private threads to non-members |
| **SMR/Intern ensure+invite adapters** | Control planes create threads and invite; `run_id→thread_id` lives in SMR |
| **Control-plane firewall** | Messaging never becomes ensure/pause/wake |

### P1 — Intern + research program

| Feature | Why |
|---|---|
| **Judgment / ask threads** (Effort-scoped, no live run required) | Async ask-and-continue UX |
| **Typed message kinds** — `ask`, `answer`, `steer`, `notice`, `blocker`, `handoff_ping` | Agents + FE can filter without NLP |
| **@mention / directed wake** — notify exact participant; soft-wake Async Effort or Sync session | Buzz `buzz-acp` pattern without owning Temporal |
| **Open-questions queue** projected from unanswered `ask`s | Effort board “Needs judgment” |
| **Sync ↔ Async shared thread** | Escalate to desk without losing context |
| **Link messages → evidence / work products / run ids** | Talk attached to research artifacts |
| **Presence / typing (lightweight)** | Humans see Async “thinking” vs idle on a thread |
| **SDK + public MCP + FE Messages** on Effort + Sync desk | Downstream lockstep |

### P1 — Factory / SMR / actors

| Feature | Why |
|---|---|
| **Ensure swarm thread** labeled `scope=run` (correlation only; SMR owns map) | Migration without run-as-room |
| **Swarm-wide broadcast vs task-directed** | Replace ad-hoc audience kinds cleanly |
| **Human steer into running swarm** (same API as Intern steer) | One operator mental model |
| **Delivery receipts / dead-letter visible in thread** | Debug “actor never got it” |
| **Factory ops channel** (start/pause factory notices as system messages) | Optional; audit-friendly |

### P2 — Collaboration quality

| Feature | Why |
|---|---|
| **Thread topics / titles + search** | Multi-Effort orgs drown otherwise |
| **Reactions / ack emoji** (minimal) | Cheap human→agent signal without a new message |
| **Pinned messages / decisions** | Magi/decision receipts linkable from thread |
| **Side threads / reply trees** | Keep Effort main thread readable |
| **Attachments** (files, visuals refs — via existing SMR media, not new blob store) | Data-heavy Sync |
| **Read receipts / last-seen cursor per participant** | “Did Async see my answer?” |
| **Mute / notify policies** per thread | Overnight Async noise |
| **Export thread → overnight artifact / report appendix** | Basis-style inspectability |

### P2 — Agent ergonomics

| Feature | Why |
|---|---|
| **Agent-facing CLI/MCP verbs** (`mq.publish`, `mq.tail`, `mq.answer`) | Same as humans |
| **Structured ask schema** (options, deadline, Effort id) | WP-AC resolve exact question |
| **Correlation ids** across thread ↔ Intern command ↔ swarm message | Support debugging |
| **Rate limits + backpressure** per participant kind | Runaway agent spam |
| **Redaction / secret scrub** on publish | Don’t leak keys into thread history |

### P3 — Maybe later / maybe never

| Feature | Note |
|---|---|
| Full Slack clone (huddles, canvas, emoji culture) | Buzz-shaped product; not Synth core |
| Voice / video | Out of band |
| Cross-org / public communities | Hard no without a different product |
| Nostr / external relay federation | Explicit non-goal |
| Chat-driven Factory scheduler | MQ must not become control plane |
| Replacing meta-thread spine with MQ history | Spine stays meta-state; MQ is talk |
| Global “all agents in one room” without scope | Security anti-feature |

### Example flows (feature combos)

**Judgment overnight**

```
Async → mq.publish(ask, effort=E, thread=T)
Human FE ← open-questions queue
Human → mq.publish(answer, correlation=ask_id)
Async ingress → resolve Effort parked question (WP-AC) → continue
```

**Live dig**

```
Async opens Sync escalate with thread=T
Human + Sync share T
Sync may mq.publish(steer) to run actors on linked run
All history stays on T for Effort Knowledge
```

**Operator steer swarm**

```
Human FE → mq.publish(steer, run auto-thread)
Delivery worker → actors (today’s path)
Receipts visible on same thread Async is watching
```

---

## Non-goals

- MQ as ensure / pause / budget / lease / generation CAS
- Peer-to-peer agent gossip outside org policy
- Multi-tenant “global hive” across orgs
- Replacing MetaHarness / spine handoffs with chat (spine = meta-state; MQ = talk)
- Intern-specific fork of MQ (must stay the shared service)

---

## Open questions (for the MQ refactor owner)

1. Thread primary key vs run message id migration strategy (backfill vs lazy auto-thread)
2. Default: one Effort ↔ many threads, or one “main” Effort thread + side threads?
3. Who may create cross-Effort threads?
4. Retention vs Intern overnight artifacts / memory search overlap
5. MCP tool naming: `research_mq_*` vs `manderqueue_*`

---

## References

- **This repo (canonical plan):** `plans/PLAN.md` · https://github.com/synth-laboratories/manderqueue
- Backend mirror / pointer: `backend/plans/smr/manderqueue_global_fabric.md`
- Current Intern×MQ (Python): `backend/plans/smr/intern_manderqueue_handoff.md`
- Intern cutover (prerequisite): `backend/plans/smr/intern_async_24_7_change_scope.md`
- Buzz (participation UX reference only): https://github.com/block/buzz · https://block.xyz/inside/introducing-buzz-where-humans-and-agents-work-together
