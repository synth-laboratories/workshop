# Handoff — live codex activity stream for Intern + SMR swarm actors

**Date:** 2026-08-08
**Goal:** tap every codex we run (the Research Intern's own codex *and* every SMR swarm actor), proxy the events through one fan-out, and expose them over an **internal-only** API so evals, acceptance tests, and local dev can watch tool calls and reasoning live.
**Audience:** subagents implementing this. Each workstream below is independently assignable.
**Repos:** `backend`, `synth-ai`, `evals`, `testing`, `synth-dev`.

Everything cited as `file.py:line` was verified by reading source in this session. Inferences are labelled. Two items are unresolved spikes — see §6.

---

## Deferred / product (2026-08-08)

- **WS1 `_put_event` tap** — deferred. Live Intern activity publishes through the
  shared `publish_codex_activity` choke point (execution-scoped Redis stream)
  injected via `AgentRuntimeExecutionRunner(on_event=…)`. Revisit the tap only
  if that path proves impractical.
- **Raw reasoning text** — product decision. Default remains **normalized**
  transcript events (same as the swarm transcript lane). Changing this is one
  edit inside `publish_codex_activity` redaction/normalize.

---

## 1. Verdict, in five lines

1. **One tap covers everything we care about:** `AppServerCodexSession._put_event` — `packages/horizons/actors/codex/session.py:994`.
2. **The Intern is blind today because of its *consumer*, not its capture.** Same spawn as swarm actors; different reader.
3. **Trace V5 cannot be the transport** — structurally post-hoc — but its wire format and cursor protocol should be reused verbatim.
4. **The internal API has an established namespace, auth dep, and schema-exclusion mechanism.** Follow them; do not invent.
5. **No acceptance proof may depend on this stream.** It is evidence, not a pass bar. §5 explains how to get visibility without breaking the interface rule.

---

## 2. Why the Intern shows nothing and swarm actors do

Codex is spawned identically for both. The divergence is the consumer of `stream_events()`:

| | Intern | SMR swarm actors |
|---|---|---|
| Consumer | `AgentRuntimeExecutionRunner._collect_session_events` — `packages/horizons/actors/runner.py:629` | `WorkerParticipantEventBridge._run` — `packages/horizons/actors/runtime.py:726` |
| Wired at | `services/intern/runners/hosts.py:668` | `services/horizons_private/runtime/backends.py:1813` → bridge `actors/runtime.py:2059` |
| Behaviour | **Completion-only.** Accumulates into `raw_messages_buffer` (`runner.py:638`), returns a tuple at `runner.py:405`. Nothing reads it mid-run. | **Per event.** Immediately hits `_emit_event_log` (`:1126`), `_emit_transcript_events` (`:950`), `_persistence.enqueue_event` (`:757`). |
| Durable result | one `intern.actor_transcript.v1` blob at teardown — `services/intern/runners/hosts.py:754-770` | per-event log records, `ParticipantEvent` rows, transcript stream — `services/smr/runtime/transcript/stream.py:206` |

`packages/intern/**` contains **zero** codex references (enforced by `tests/units/test_intern_runtime_actor.py:973`); the Intern builds a `CodexProfile` at `services/intern/runners/hosts.py:916` and hands it to the same horizons hosts.

**Consequence:** a tap at or below `_put_event` is upstream of this split and fixes both at once. A tap anywhere downstream fixes only swarm actors.

### The production dispatch path — and a large dead branch beside it

`POST /smr/projects/{id}/trigger` (`app/api/v1/managed_research/runs.py:303`) → `run_start/launch_service.py:136` → `trigger_service.py:690` → `service.py:886` → run row → `service.py:1060` bootstrap authority → Redis effect → `launch_bootstrap_effect_worker.py:58` → `service.py:2095` → **Temporal `SmrRunWorkflow`** (`temporal_supervisor.py:110`). Supersteps are driven either by the Temporal activity (`activities/smr.py:822`) or, on the hot path, the in-process Horizons ticker (`runtime_driver.py:419` → `horizons/runtime/service.py:1378` → `smr_bridge.py:1590`). Launch commands flow `state_machine_runtime.py:2033` → `command_application.py:3316` → `adapters/actor_runtime.py:2969` → `:4243 _launch_assignment` → `assignment_launch_authority.py:906` → `horizons/runtime/participants.py:3113` → `horizons/actors/runtime.py:1890 start_participant_session` → `event_bridge.start` (`:2140`).

There is **no warm actor pool**; each participant session resolves a fresh host by `host_kind` (`services/horizons_private/runtime/hosts.py:123`). "dispatch_pool"/"worker_pool" is a routing label on a Redis ready-actor ZSET, not a container pool. A Factory run *is* a swarm run — same row, plus `effort_id` and a server-minted `factory_wake_confirmation` (`services/smr/api_services/factories.py:11265`).

> **⚠️ Do not build or test against `worker_loop`.** The run-scoped harness path (`worker_loop.py:2599` → `task_turn_activity.py:341` → `codex_runtime.py:85` → `execution/codex_turns.py:63`) is gated on `SMR_LEGACY_RUN_SCOPED_HARNESS_ALLOWED`, which **defaults to off** — `legacy_run_scoped_harness_allowed()` returns falsey for an unset var (`services/smr/runtime/participant_session_authority.py:180`), and `worker_loop.py:989` short-circuits with `legacy_run_scoped_worker_host_bootstrap_disabled`. **Verified.** A fixture that drives `worker_loop._execute_run_local_task_turn` exercises dead code and will pass while production emission is untouched. Also retired: `services/smr/actor_pool/api.py:1`, `leader.py:1`, and the `worker_loop.py:645` stub. Correspondingly, do **not** bother wiring `record_task_codex_event` (`services/smr/persistence/task_execution.py:903`) — it only ever fires on the dead path.

### Also true, and relevant to the acceptance cells

- **The async Intern never publishes a Trace V5 at all.** `_record_terminal_sync_trace_publication_after_close` is called only from the sync path (`services/intern/runtime_authority.py:1808`, guard at `:1492`/`:1550`); there is no async equivalent. So on `A-*` cells the `trace_evidence` proof is satisfied entirely by *swarm actor* traces.
- **Swarm worker turns never reach the Intern ledger, by design.** `_transcript_runtime_correlation` (`packages/horizons/actors/runtime.py:978`) returns `{"status": "unbound"}` when there are no tracked interaction deliveries — the normal case for a plain worker actor — and that marker is what gates the Intern projection at `durable_writer.py:230-252`. So an Intern-ledger observer can never see swarm actor work; the two planes must be joined by the consumer, not the backend.
- **Intern stderr is hashed, not retained** — `services/intern/runners/hosts.py:761` stores per-line `sha256` only. Live stderr content requires changing that transcript contract (product decision).

---

## 3. What is available at the tap, and what is redacted

Codex app-server speaks newline-delimited JSON-RPC (`packages/horizons/actors/codex/stdio.py:325-406`). **Nothing is discarded at the protocol layer:** `_handle_notification` sets `event_payload = dict(message)` (`session.py:1583`), and `CodexSessionEvent.payload` carries the full raw message. Kinds classified at `session.py:5189`: `agent_message`, `reasoning`, `command_execution`, `file_change`, `mcp_tool_call`, `item/started|updated|completed`, `turn/*`, `thread/*`.

Downstream lanes each drop something different — this is why the tap must be at `_put_event`, before any of them:

| Lane | Drops |
|---|---|
| Runtime logs (`actors/runtime.py:1126`) | `AGENT_MESSAGE`, `REASONING`, `ITEM_*`, `FILE_CHANGE` (filter `runtime.py:3269`) |
| Participant events (`_map_codex_session_event`, `runtime.py:2718`) | nothing — full raw payload (`runtime.py:3243`) |
| Transcript / live-trace (`services/smr/runtime/transcript/normalization.py:63`) | **raw reasoning, deliberately.** `/delta` and `/updated` return `None` (`:274`); non-summary reasoning replaced with `"Synth removed it and kept only product-safe output"` (`:246`, `:288`) |

**Credential redaction is always-on, pre-spool, and fails closed** — `packages/smr/runtime/execution/trace_capture.py:119-231`; env-shaped mappings redacted wholesale (`:190-204`); `Bearer …` / `sk-…` / `mcp-…` scrubbed (`:130-137`). Headers are dropped, not masked. **Preserve this.** It is not optional and you get it for free by tapping after normalization *only if* you choose that point — at `_put_event` you are *before* it, so the new sink must apply redaction itself. See §6 spike 2.

**Grading isolation is about authority, not visibility.** Enforced by which root is graded (`evals/core/harbor/runner/native_harbor_trace_v5_runner.py:308-329`) and which reviews count (`evals/core/dock/trace_v5.py:315-323`) — not by blinding traces. So there is **no held-out reason** to withhold a live stream from evals. Constraints that do apply: keep credential redaction, keep the contamination gate's premise that one capture = one actor's calls (`evals/core/harbor/tracing/trace_v5.py:222-310`), and never let the live channel become terminal authority for a verdict.

---

## 4. Workstreams

### WS1 — Capture tap, **Intern only** (backend, `packages/horizons`)

**Scope narrowed 2026-08-08:** swarm actors already emit per event (see WS2), so this tap is needed only if WS2 option (1) — reusing the existing publisher for the Intern — proves impractical. Read WS2 first.


Add an optional observer callback to `AppServerCodexSession.__init__` (`packages/horizons/actors/codex/session.py:564`), invoked at the top of `_put_event` (`:994`).

**Hard requirements:**
- **Must never raise and never block.** An exception in `_put_event` today terminates the codex process and fails the session (`session.py:996-1048`). Wrap in try/except, drop on error, count drops.
- **Must not consume `SessionEventBuffer`.** `get()` *pops* (`packages/horizons/actors/event_buffer.py:338`) — a second consumer steals events from the real one. Limits are hard-fail: 65,536 events / 16 MiB / 256 MiB spool (`packages/horizons/actors/limits.py:7-12`), overflow raises `CAPACITY_EXCEEDED` (`event_buffer.py:301`), and `assert_admission_available` refuses new turns above 75% (`event_buffer.py:396`). Fan out, do not read.
- Do **not** wrap `stream_events()` — the continuation decorator precedent (`packages/horizons/actors/materialization/continuation.py:227`, forwarding at `:66`) is single-consumer and cannot fan out.

**Wire the same callback for both consumers** so Intern and swarm actors are covered: `services/intern/runners/hosts.py:668` and `services/horizons_private/runtime/backends.py:1813`.

**Done when:** an in-process subscriber receives every codex event for both an Intern execution and a swarm actor turn, with the session unaffected when the subscriber raises or is slow.

### WS2 — Transport: reuse the per-run transcript stream (backend, `services/`)

**Corrected 2026-08-08 after the swarm-dispatch review. The swarm half of this is already built.**

Swarm actors already publish **per event** into a **per-run Redis Stream**, and the read side is already cursored, blocking, and consumer-group capable:

| Piece | Where |
|---|---|
| Publish (per event, live) | `WorkerParticipantEventBridge._run` — `packages/horizons/actors/runtime.py:720`, `stream_events()` at `:726`, `_emit_transcript_events` at `:745`, `publish_codex_session_event` at `:961`; publisher wired at `services/horizons_private/runtime/backends.py:1832` |
| Normalize + publish | `services/horizons_private/runtime/smr_transcript.py:110-146` → `publish_live_events` (`services/smr/runtime/transcript/stream.py:206`) |
| Stream key | `transcript_stream_key(run_id)` — `services/smr/control_plane/transcript_stream.py:674`. **Per run**, not global |
| Cursored read | `list_live_event_entries_strict(run_id, after_cursor, limit)` — `services/smr/runtime/transcript/stream.py:280` |
| Blocking read | `read_live_event_payloads(... block_ms=)` → `redis.xread({key: cursor}, block=…)` — `control_plane/transcript_stream.py:863-885` |
| Consumer-group fan-out | `read_consumer_group_payloads(run_ids, consumer_name, group_name, count, block_ms)` — `runtime/transcript/stream.py:370` |

**Critically, this does *not* have the global-stream flaw of the Intern SSE plane** (`services/intern/runtime_events.py:44`, `:189-193`), because it keys on `run_id`. Subscriber cost scales with the run you care about.

**The only consumer today is `services/smr/runtime/transcript/terminal_archive.py:322`.** There is no live API surface over it. That is precisely the gap WS3 fills — so for swarm actors, **WS3 is a thin SSE adapter over `list_live_event_entries_strict`, and WS1/WS2 need no new capture or transport at all.**

#### What is actually missing: the Intern side

The Intern has no equivalent because its consumer is completion-only (§2). Two options, in preference order:

1. **Preferred — make the Intern's consumer per-event, reusing the same publisher.** Wire `SmrCodexTranscriptEventPublisher` (import at `services/horizons_private/runtime/smr_transcript.py:204`) into the Intern's runner at `services/intern/runners/hosts.py:668`, mirroring the swarm bridge. One publisher, one schema, one transport, one API.
2. Fallback — the WS1 `_put_event` tap, if (1) proves impractical.

**Open design decision for either option:** the transcript stream is keyed by `run_id`, but the Intern's codex execution is keyed by `execution_id` (`services/intern/runners/hosts.py:754-770`) and the async Intern is not necessarily inside a Factory run. So you must either introduce a second key space (per-runtime / per-execution) or bind the Intern's stream to the run it triggered. Decide this **before** WS3, since it determines the endpoint's path parameter. Do not paper over it by reusing an unrelated `run_id`.

**Note:** the Context Engine live-trace plane (`services/smr/context_engine/live_trace.py:52`, `publish_live_trace_batch`) is a *second*, **opt-in** channel gated by `CONTEXT_ENGINE_LIVE_TRACE_ENABLED`, default **off** (`:24-31`). Its batching design is worth reading as a pattern, but it is not the transport and not a dependency.

### WS3 — Internal API (backend, `app/`)

**New file:** `app/api/v1/managed_research/internal_codex_activity.py`

```python
router = APIRouter(prefix="/smr/internal/codex-activity", tags=["smr-internal"])

@router.get(
    "/stream",
    include_in_schema=False,                      # MANDATORY — see below
    response_class=StreamingResponse,
    dependencies=[Depends(require_worker_key)],   # X-SMR-Worker-Key
)
async def stream_codex_activity(request: Request, org_id: str | None = Query(None)):
    # Always on; worker-token gate only (no env enable flag).
    await audit_event(db, org_id=org_id, actor="internal_observer",
                      actor_kind="smr_worker_service_token",
                      event_type="smr.internal.codex_activity.stream",
                      source_ip=request.client.host if request.client else None)
    ...
```

| Decision | Value | Why / precedent |
|---|---|---|
| Path | `/smr/internal/codex-activity/stream` | free today; inherits the worker-token namespace at `app/api/v1/managed_research/internal.py:43`. **Mounted at `/smr`, never `/api/v1`** (`app/api/v1/smr_internal.py:1`) — say so in the docstring; `evals/.../smr_metered_infra.py:17-25` force-appends `/api/v1` and targets a path that does not exist |
| Mount | register in `mount_routes`, `app/api/v1/smr_internal.py:11-13` | one edit reaches both the full app (`app/api/app.py:1132`) and the lean local-slot app (`app/api/local_app.py:153`) — a slot-local observer needs both |
| Auth | `Depends(require_worker_key)` — `services/smr/api_dependencies.py:183-220` | dominant `/smr/internal/*` convention (`services/smr/api_services/internal.py:117-118`). **Avoid `ValidatedAPIKeyOrSmrWorkerKey`** — it also admits any customer key, defeating "never by customers" |
| Env gate | **none** — stream is always mounted; worker token is the only gate | product decision 2026-08-08: do not require `SMR_INTERNAL_CODEX_ACTIVITY_STREAM_ENABLED` (removed). Auth + `include_in_schema=False` keep it off the customer surface |
| Schema exclusion | `include_in_schema=False` **and** never add to `PUBLIC_OPERATION_IDS` | two independent layers. `smr_openapi.yaml` is an *unfiltered* dump (`scripts/validate_smr_openapi.py:254-263`) **vendored into the SDK repo**; the flag is the only thing keeping `/internal/local` out of it (0 occurrences vs `/smr/internal` 47×). `research_openapi.json` copies only allowlisted pairs (`scripts/export_research_openapi.py:78-113`, allowlist `app/api/v1/managed_research/openapi_contract.py:12-730`) |
| Cursor protocol | `after_ordinal` query param, `Last-Event-ID` header **wins** over it | copy `services/smr/runtime/public_trace_stream_service.py:281-310`; SSE frames `id: {capture_id}:{ordinal}` |
| Cross-org | audit **before** the query | copy `services/smr/admin_service.py:60-85` (`admin_list_tenants`), helper `services/smr/api_helpers.py:45-73`. Do **not** copy the dashboard-growth pattern (`routes_internal_dashboard_growth.py:93-101`) — unscoped and unaudited. If scoping is wanted, reuse the fail-closed allowlist at `services/smr/api_dependencies.py:113-146` |

**Consider implementing the pre-declared slot instead of/alongside a new path.** `get_project_run_actor_trace` already accepts `cursor`, `live_cursor`, `include_live` and **literally discards them** — `services/smr/runtime/public_run_observability_service.py:1613` is `del cursor, live_cursor, include_live`, with `events=[]`/`live_events=[]` hardcoded at `:1665-1666`. The contract shapes and SDK surface already exist. Tradeoff: that route is customer-key/operator-gated, so it cannot be the internal cross-org surface — but it is the natural home for the *public* half if you ever want acceptance cells to consume this legitimately (§5).

**Tests to add** (mirror `tests/units/test_internal_local_registry.py`): pin the route set, assert both app factories mount it, and add a category-absence test modeled on `tests/units/test_intern_no_craftax_surface.py:114-140` asserting the paths appear in **neither** `PUBLIC_OPERATION_IDS` **nor** `research_openapi.json`. No such absence test exists today.

Also add an `OPERATOR_SURFACES` entry — `app/api/debug_manifest.py:16`, surfaced at `:190`.

### WS4 — Consumer + credentials (evals, testing, synth-dev)

**The big win: if WS2 emits `synth.capture.raw.v1` framing, the consumer already exists.** `evals/core/streaming/source_trace_v5.py:16-86` (`ContainerTraceSource`) already speaks `after_ordinal` cursoring and finishes on `page.closed and after_ordinal >= high_water_ordinal` (`:82-86`). Point it at the live endpoint instead of the spool and you write **zero** new consumer code.

**Credential plumbing — activate an existing hook, do not invent one:**
- Callers under `slotctl eval-exec` need **nothing**: `synth-dev/local_dev/slot-manager-rs/src/artifacts.rs:31` already projects `EVAL_EXEC_SECRET_ENV_KEYS = ["SMR_WORKER_API_KEY", "SYNTH_API_KEY"]`.
- Pytest callers have no worker credential: `testing/tests/synth_dev.py:14-26` — `LocalSynthDevTarget` has `api_key` and no worker field. Add `worker_api_key`, read from the same `slot.env` the `api_key` comes from (`:126-146`), with `SYNTH_EVAL_EXEC_WORKER_API_KEY` as its override — a name the slot manager already honors (`artifacts.rs:1158-1161`) but which has **zero** consumers today (verified).
- The worker key is **Railway-homed and fingerprint-pinned** (`synth-dev/shipping/secrets/seed.py:33-38`) and is a forbidden host-env shadow key (`shipping/secrets/canonical_shadow_keys.toml:14`) — a host `export SMR_WORKER_API_KEY=…` fails preflight. Do not try to fabricate it locally the way `SYNTH_API_KEY` is (`artifacts.rs:15`).

**Note the two header shapes in existing eval callers** and state the chosen one in the endpoint docstring, or callers will guess: `X-SMR-Worker-Key` (`evals/suites/product/swarms/reportbench/suite.py:207-247`) vs `Authorization: Bearer` (`.../lifecycle.py:332`). They share no helper.

### WS5 — Wiring into the Intern acceptance tests (testing)

**Read §5 before starting this one.** The short version: the matrix runner may consume the internal stream for operator visibility; the cell's driver may not, and no proof may depend on it.

Concretely:
- `testing/acceptance_tests/intern_247_launch/bin/tail_intern.py` exists and already streams the *public* Intern ledger (tool name, full arguments, model-authored rationale, checkpoints, `agent_message` prose) under the cell's customer key. Wired as `--stream-intern` in `bin/run_matrix.sh`. **Keep this as the acceptance-safe path.**
- Add a **second, opt-in** codex-level observer (`--stream-codex`) that the *runner process* launches with the worker key, writing to `runs/<stamp>/cells/<cell>/codex-stream.log`. The driver subprocess must not receive the worker key.
- Do **not** add `SMR_WORKER_API_KEY` to `apply_cell_org_env` or to the minted-org `.env` files. `run_matrix.sh` currently `unset`s sibling credentials deliberately (`:1447-1476`).
- Do **not** add any proof id backed by the codex stream. Proofs come from the driver.

---

## 5. The constraint that shapes WS5

`backend/AGENTS.md:35-49` and `.cursor/rules/acceptance-sdk-or-frontend-only.mdc` (`alwaysApply: true`):

> **HARD RULE.** Intern / Managed Research / launch acceptance drives the product **only** through natural `SynthClient` SDK usage or the local frontend. **Never:** … importing `services.*` / `packages.*` as the test actor. Cannot express it as SDK or FE → **fix the product, do not invent scaffolding.**

`testing/acceptance_tests/intern_247_launch/INTERFACE.md:9,45` defines the `api` surface as "**public** Intern SDK/HTTP", and `bin/check_interface_policy.sh` fails closed on anything else.

**What this forbids:** the internal API being the pass path of any acceptance cell.
**What it permits:** diagnostic/evidence use — explicitly outside the enforcer's scan scope.

So the resolution is a clean split, and it is a *design* answer rather than a workaround:

| Consumer | Credential | May proofs depend on it? |
|---|---|---|
| evals, local dev | worker key | yes |
| acceptance **runner** (operator visibility) | worker key, runner process only | **no** |
| acceptance **cell driver** (the test actor) | minted-org customer key only | n/a — cannot reach it |

The acceptance cells holding only a customer key is the mechanical expression of that rule, not an oversight to route around. If a cell genuinely needs this data as a pass bar, `AGENTS.md:47-49` says the answer is to expose it on the **public** SDK surface — which is what the `get_project_run_actor_trace` stub (§WS3) is already shaped for.

---

## 6. Spikes — verify before building on them

1. **Does reasoning *text* survive codex capture end to end?** Token counts always do (`evals/core/dock/trace_v5.py:173`). Whether un-summarized reasoning text is on the wire depends on `synth-containers` internals, which no reviewer audited, plus codex's own reasoning-summary policy. The app-server tap sees codex's *view*; the provider wire is only visible at the responses proxy. **Resolve by running one cell and dumping raw `_put_event` payloads for `kind == reasoning`.** If text is absent there, the second tap is `core/o11y/codex_responses_proxy.py:388` — which already yields chunks incrementally (`:387-389`) but today only accumulates into one terminal OTLP span (`:403`), is gated behind `SMR_TRACING_CAPTURE_CONTENT` (`docker/runner.py:195`), and **is wired only by the Docker runner** — so Intern async (sticky exe.dev, `services/intern/runners/hosts.py:374`) is not behind it at all. Extending `_apply_codex_responses_capture_config` to exe.dev/Daytona/local is a real piece of work.
2. **Where does redaction happen relative to the new sink?** At `_put_event` you are *upstream* of `trace_capture.py:119-231`, so the new sink must apply the same redaction itself or it will publish credential-bearing payloads. Confirm by diffing a `_put_event` payload against the corresponding spool segment. **This is a security gate, not a nicety** — the library-side contract fails closed on secret shapes (`synth_containers/tracing/capture/redaction.py:1-5`) and the new path must not be the hole.
3. **`resource_kind="swarm"` vs `ResourceKind.RUN` binding gap.** `packages/intern/core/model.py:81-85` and `services/intern/smr_mcp_dispatch.py:551-566` disagree on the binding kind. A streamer keyed on `binding.run_id` inherits the discrepancy. Targeted test before keying anything on it. *(inferred — flagged by the swarm review, not independently confirmed.)*
4. **Exposing raw reasoning to evals is a product decision.** The transcript lane redacts it deliberately (`normalization.py:246`). Get an explicit call before surfacing it, even internally.

### Out of scope for this handoff

- **Project-computer codex sessions.** A second launcher copy exists (`packages/smr/sandbox/codex_session.py:50` → `codex_stdio.py:223`), reachable via `packages/smr/sandbox/runtime_adapter.py:243` ← `services/smr/project_computers/api.py:59`. I verified `services/intern/**` and `packages/intern/**` contain **no** `project_computer` references, and `services/smr/session_runtime/` has zero references outside its own directory (no compose, no Dockerfile, no importer). **So one hook at `_put_event` covers everything the Intern acceptance tests exercise**, but this second path is uncovered if you later want it.
- **Harbor `codex exec --json`.** `services/container_pools/rhodes/harbor_runtime.py:1224` writes `/logs/agent/codex.jsonl` inside the sandbox, tarred and uploaded **after** the verdict (`:381-400`). Zero live. Covering it means `tail -f` during the rollout or pointing codex at the responses proxy.

---

## 7. Why not just extend Trace V5

Four independent structural blockers, each verified:

1. The only incremental writer targets **worker-local `/tmp`** — `packages/smr/runtime/execution/trace_capture.py:427-434`. Nothing off-host can read it.
2. Every network-addressable read is gated on a terminal `finalize`: `query` filters `status == 'committed'` (`services/smr/trace_stores/repository.py:906`); the SSE replay 409s `trace_v5_bundle_not_published` (`services/smr/runtime/public_trace_stream_service.py:62-70`).
3. The catalog is **append-only by database trigger** — `alembic/versions/20260802_trace_v5_catalog.py:534-547`. A growing trace is prohibited by the storage layer's integrity model, not merely unimplemented.
4. No tailing semantics, and polling is taxed: no cursor on `query`, an access-receipt DB write per call (`services/smr/trace_stores/service.py:2276-2288`), and an O(all-traces) blob-download fallback when `actor_id` is filtered without Turso (`:2172-2216`).

Forcing it means relaxing `committed`, dropping the immutability triggers, and re-sealing mid-flight — which breaks digest-is-identity, breaks the reconciliation-receipt requirement, and destroys the contamination gate's ability to say "this capture is exactly this actor's calls." Trading a working evidence system for a mediocre message bus.

**Keep this convergence guarantee:** the live channel must be rebuildable and reconcilable to the sealed bundle — same `capture_id`, same `ordinal` space — and never terminal authority for a verdict. Mirrors `evals/core/streaming/stream.py:353` ("Attach trace facts without transferring terminal authority") and the `drain_after_terminal=True` hybrid at `evals/scripts/run_bench_matrix.py:5321-5330`.

---

## 8. Sequencing

```
WS1 capture tap ──┬─► WS2 fan-out + envelope ──► WS3 internal API ──► WS4 consumer ──► WS5 acceptance wiring
                  │
                  └─► spike 1 (reasoning text?) and spike 2 (redaction) run against WS1 immediately
```

WS1 + both spikes first — they are cheap and they decide whether WS2 needs the responses-proxy second tap. WS3 and WS4 can be built in parallel once WS2's envelope shape is frozen. WS5 last, and it is the smallest piece.

**Two systemic gaps worth escalating separately from this work:** `include_in_schema` is applied inconsistently today, so `/openapi.json` (and the spec vendored into the SDK repo) already exposes `/smr/internal/*` 47×, `/smr/admin/*` 12×, and `/api/v1/internal/*` — only `research_openapi.json` is bounded. And a single line added to `PUBLIC_OPERATION_IDS` publishes a route to the SDK with no gate objecting; review of that dict *is* the access-control boundary.
