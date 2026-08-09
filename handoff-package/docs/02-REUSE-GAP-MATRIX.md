# Reuse / Gap Matrix

Answers the reviewing-agent questions from the product handoff against live `backend`, `synth-ai`, and `frontend` (reviewed 2026-08-08).

---

## A. Concept mapping

| Desktop concept | Existing Synth equivalent | Reuse path | Gap |
|-----------------|---------------------------|------------|-----|
| **Session** | Sync: `SyncSessionResponse` (`smr.intern-sync-session.v1`); Async: org singleton `AsyncRuntimeResponse` (`smr.intern-async-runtime.v1`); Local: none | Map both Intern planes + local into desktop `Session` | Need desktop Session ID that can wrap sync_session_id / async runtime / local uuid |
| **Run / InternRun** | Sync turns + Async cycles (`cycle_number`); SMR Project/Run/Swarm are *bound* resources, not Intern itself | Use Intern projection status + event ledger as Run lifecycle | Desktop `InternRun { mode, status, latestCursor }` is a thin facade |
| **Event stream** | `InternRuntimeEventResponse` + SSE `smr.intern-runtime-event-stream.v1`; cursor = `after_sequence` / `Last-Event-ID` | `runtime_events.py`, FE `openInternRuntimeEventStream`, SDK `events`/`tail`/`stream_events` | Local Laguna must emit **compatible** events (or daemon normalizes) |
| **Artifact** | SMR artifacts + Sync visuals/experiments/harness-bundle; `ArtifactFrame` in FE | Linked GETs from resource_refs on events | Local artifacts (files, charts) need content-addressed store in daemon |
| **Metric** | Async spend/budgets; Sync usage endpoints; timing on inference not unified | Reuse spend for Intern; invent local tok/s + latency metrics | Unified `Metric` on RolloutStep is new |
| **Rollout** | `smr_pool_rollout` (eval/pool) — **not** Intern-primary | Do not force Intern into pool rollout yet | Desktop rollout = ordered Event log + step index for V1 |
| **Harness / Environment** | Sync harness-bundle download; Factory/Environment in SMR | Optional attach later | Full Harness Revision deferred |
| **Model** | Cloud: `poolside/laguna-s-2.1` etc. in Codex catalogs; Local MLX: **missing** | Reuse naming for identity; separate `adapter` field | Local MLX lifecycle entirely new |
| **Checkpoint** | Async `request_checkpoint` + projection checkpoint; Sync turn projection | Async checkpoint commands | Local checkpoint = conversation snapshot |
| **Approval / Interaction** | Sync presence + `answer_interaction`; Async parked questions / `provide_input` | FE `useSyncPresence`, SyncApprovalPanel patterns | Desktop device presence id |
| **Local agent host** | **Local Pilot** (`services/intern/local_pilot.py`) — loopback attach/context/commands | Closest “desktop drives Sync turn” precedent | Wire Electron as pilot host (optional M3+) |
| **Codex App Server** | Remote guest: `packages/horizons/actors/codex/*` over exe.dev SSH | Reference for NDJSON-RPC if embedding Codex later | **Not** V1 desktop ontology |
| **ACP** | Daytona `sandbox_agent` `/v1/acp` | Deferred | — |
| **Electron** | **None** in backend/frontend/synth-ai | Greenfield `apps/desktop` | — |

---

## B. Which backend becomes the local runtime daemon?

| Candidate | Verdict |
|-----------|---------|
| Rhodes / full backend | **No** — too heavy; cloud authority stays remote |
| Horizons runtime | **No** as desktop host — remote VM actor plane |
| New thin daemon | **Yes** — owns local sessions + adapts to remote Intern HTTP |
| Local Pilot | Optional mode where daemon *participates* in Sync turns |
| understudy | Optional local exe.dev stand-in; not required for V1 Intern-as-client |

**Recommendation:** New `services/local-runtime` (Python) depending on `synth-ai`, talking to hosted/local backend URL. Electron never calls Intern HTTP directly.

---

## C. Closest existing protocol to the required event model

**Winner:** Intern runtime mailbox events (`after_sequence` + generation-fenced commands + receipts).

```text
WRITE: InternRuntimeCommandRequest
READ:  projection + events
LIVE:  SSE after cursor
```

Law: MCP ≡ SDK ≡ HTTP ≡ same mailbox (`plans/intern_interaction_boundaries.md`).

Do **not** use manderqueue as the client↔Intern API. Do **not** treat internal Codex activity SSE as product authority.

---

## D. Managed Research persistence → Desktop sessions

| Intern | Desktop mapping |
|--------|-----------------|
| Many Sync sessions | Desktop Sessions with `target.mode=sync`, durable `sync_session_id` |
| One Async runtime / org | Desktop “Background Intern” panel; reconnect by org, not by many IDs |
| PG event ledger | Source of truth when online; daemon caches pages for offline UI |
| Presence lease | Desktop connection/device lease for approvals |
| Disconnect ≠ pause (Async) | Critical UX: closing app must not pause Async |

---

## E. What to reuse from existing code

### Backend
- `packages/intern/contracts.py` — projections, receipts
- `packages/intern/mailbox/*` — cursor/command/event protocol shapes
- `services/intern/runtime_events.py` — SSE semantics
- `services/intern/local_pilot.py` — local turn execution hook
- `app/api/v1/managed_research/research_intern.py` — HTTP surface
- OpenAPI allowlist / `research-v1.json`

### SDK (`synth-ai`)
- `SynthClient().research.intern.sync_` / `.async_`
- Contracts in `sdk/research/contracts/research_intern.py`
- MCP tools `intern_sync_*` / `intern_async_*` if daemon wants tool-shaped control
- **No TypeScript client** — generate from OpenAPI or call via daemon

### Frontend
- `src/lib/researchIntern.ts` — **highest leverage** (types + SSE replay/tail)
- Sync projection helpers (`syncSessionProjection`, `syncProductEvents`)
- `useSyncPresence.ts`
- UX reference: SyncCockpit layout, EffortBoard (Async primary product UI)
- `ArtifactFrame`, visuals workbench components (extract presentational)
- Chrome tokens (`smrChrome`) — optional; desktop may restyle

### Do not reuse as-is
- Next.js `/api/smr/**` BFF
- Clerk cookie auth path
- Monolithic `SyncCockpit.tsx` without splitting
- Deprecated Async assignment CRUD as primary UX
- Legacy Intern `/sessions` plane

---

## F. Content-addressed artifacts?

Partial: SMR hosted artifacts + Sync harness bundles exist. Intern events carry `resource_refs`; full content-addressed local artifact CAS for desktop is **to invent** in the daemon (hash → blob store).

---

## G. Harness Revision

Natural representation today: Sync harness-bundle + Factory/Environment bindings on `InternRuntimeBinding`. Full version graph is **deferred**; store optional harness ids on Run metadata now.

---

## H. Where Codex / ACP adaptation live

| Layer | Location today | Desktop V1 |
|-------|----------------|------------|
| Codex App Server | Remote Horizons actors (`codex/session.py`, exe.dev) | Behind Intern cloud path only — don’t embed in Electron |
| ACP | Daytona sandbox_agent | Deferred |
| Desktop adapters | N/A | Only if Local Pilot / local Codex later |

---

## I. Local inference boundary

```text
Recommended:
  Electron ─IPC─► local-runtime daemon ─HTTP─► local-inference (MLX) child/service
                                              OpenAI-compatible /v1/chat/completions stream
```

| Option | Verdict |
|--------|---------|
| Separate inference process | **Preferred** (crash isolation, Metal lifecycle) |
| Embedded native module | Avoid for V1 |
| llama.cpp fallback | Secondary; MLX first for Laguna-on-Mac |

Design `adapter: null` on every local request now even if LoRA lands in V1.1.

---

## J. Web UI shared with usesynth.ai?

**Yes for presentational React + `researchIntern` client logic.**  
**No for App Router pages / BFF.** Extract a future `packages/intern-ui` / `packages/runtime-client` rather than importing Next routes into Electron.

---

## K. Local vs Cloud as one `ExecutionTarget`

Already natural:

```text
ExecutionTarget.local  → Laguna adapter
ExecutionTarget.intern → SynthClient sync_ | async_
```

Daemon normalizes both into `Event[]` with `sequence` (local sequences are daemon-local; Intern sequences are server-authoritative — **do not mix cursors across targets**).

---

## L. Minimum subset for first shippable prototype

1. Runtime protocol types + fixture replay  
2. Electron shell + daemon IPC  
3. Local Laguna stream (even stubbed)  
4. Intern sync create/send/tail  
5. One transcript/timeline UI  
6. Persist cursors across restart  

Everything else is milestone 4+.

---

## M. Rough effort (order-of-magnitude)

| Component | Effort |
|-----------|--------|
| Runtime protocol + daemon skeleton + persistence | 1–2 eng-weeks |
| Intern sync adapter (via synth-ai) + SSE | 3–5 days |
| Intern async ensure/reconnect/pause UI | 3–5 days |
| Electron shell + Run viewer | 1–2 weeks |
| Local MLX Laguna (real) | 2–4 weeks (highest risk) |
| Local Pilot wiring | 3–5 days after Sync works |
| Artifact CAS + viewers | 1–2 weeks |
| Eval/ARIA pass | 3–5 days |

---

## N. Major risks

| Risk | Mitigation |
|------|------------|
| Laguna MLX performance / packaging | Isolate inference process; stub first; measure tok/s early |
| LoRA + DFlash acceptance drop | `adapter` field now; disable speculator per adapter later |
| Treating Sync/Async as one wire API | Keep wire separate; unify only in desktop domain model |
| Closing app pauses Async | Explicit UX + never auto-pause on disconnect |
| Generation conflicts on commands | Always send `expected_generation`; handle `conflict` receipts |
| FE BFF habits in Electron | Direct backend URL + API key in daemon only |
| Codex protocol drift | Don’t depend on Codex for V1 product path |
| Over-designing rollout schema | V1 rollout = event log + step index |
| Global Intern SSE wake cost | Known backend limitation; fine for single-user desktop |
| Auth / keychain | Never put API key in renderer |
