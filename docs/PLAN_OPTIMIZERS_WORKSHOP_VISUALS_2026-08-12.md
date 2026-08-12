# Plan: Workshop + Optimizers against Containers §12

**Status:** Final for this cut. **C7 and C8 signed off. Containers may start the floor.**  
**Answers the handoff:** [`HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md)  
**Date:** 2026-08-12

---

## Verdict

**§12 C7 is enough.** Do not invent a second stream. Do not wrap events in an outer `synth.stream-event.v1` envelope. Do not mint a signed `child_eval_ref`.

Freeze five small things in the Containers receipt so Desktop / sidecar cannot guess:

1. **Create-rollout echoes the stream descriptor** (`stream.id`, poll/SSE URLs, cursor kind, `/reward` URL). Discovery-only is not enough.
2. **Subscribe control record** (non-advancing): `stream.subscribed` with `stream.id` + `next_sequence`. HTTP 200 on GET is not a ready-ACK.
3. **Typed occupancy refusal** when `scale_leases` is exhausted. Do not silently share a world lease across two optimizer runs.
4. **Artifact retention advertised** (`run` vs TTL). Frames 404 after `world_stop` must be named, not silent.
5. **Consumer cursor is sequence.** Relay materializes a sequence log; Workshop never speaks `nev_cursor`.

If those five are in C0/C1/C7, Workshop and Optimizers can bind C7-W / C7-O without a Containers rewrite.

**A8 (dig.bench)** is the Desktop capstone on a third content family, not a new envelope. Same reducer, slot `stream`, connect-before-`start_session`. Optimizers unused on dig.bench this cut. Do not start A8 in Desktop until Containers **C8** headless passes. Harbor-wrapping their HTTP, inventing frames, or a second stream would mean A1–A7 were not actually done.

---

## Locked (do not reopen)

Same as the handoff. Extra closures from this review:

| Item | Decision |
| --- | --- |
| Envelope | Two concrete schemas: `synth.trace-stream-event.v1` (eval/child) and `optimizer_event.v1` (search/train). `synth.stream-event.v1` is a **trait** (sequence, missing≠0, heartbeat ≠ cursor, poll/SSE/WS equivalent). |
| Slot | Live eval templates bind slot **`stream`**. `live.harbor_eval.v1` today uses `jobs` — Workshop bug, we rename. C1-09 / C7-W01 must fail `live` and `jobs`. |
| `/reward` v1 | Scalar `reward` + `node_results[]`. Optional `components` ignored by visuals. |
| Child eval | `rollout_id` + declared `stream.id` + `/reward` URL as `synth.resource-ref.v1`. Optimizer creates the rollout; Containers does not sign an optimizer-shaped blob. |
| Ready | Subscribe + control ACK **before** `POST /rollouts` / first paid event (C1-08). |
| Provisional | Craftax plots log `RewardSignal`s; `/reward` is the terminal cell. Harbor: `live_reward=unsupported`. |
| Policy | `luna_med` / `sol_med` are **policy configs**, not tasks. No sibling `harness_ref`. GEPA proposer and child-eval are two `policy_ref`s. |
| `auto` transport | Forbidden on authoritative / visual-attached runs. |
| Flip vs two-board | Flip is enough. |
| Specta | Do not block. |
| Private Evals names | Stay out of public templates. |

`aug_12_update.md` §2.2 still lists `harness_ref` on create-rollout. That line is **superseded**. Recipe = `policy_ref` `{ harness, config, code? }`.

---

## Answers for Containers (handoff prompts 1–9)

### 1. Stream bind

Declared `stream.id` + poll + SSE (`Last-Event-ID`) + optional WS is enough for every template in this cut.

Two schemas only. Trait, not a third envelope. Bind the id from create-rollout / discovery; never construct `/events` vs `/rollouts/{id}/stream`.

**Need on the wire:** stream descriptor on **create-rollout response** and GET metadata:

```text
stream.id
transports.poll.url
transports.sse.url          (null if not bound)
transports.websocket.url    (null if not advertised)
cursor.kind                 sequence   (consumer-facing)
cursor.producer_kind?       nev_cursor | ordinal   (internal; do not require consumers)
reward.url                  /reward?rollout_id=…
auth.mode
```

Poll is required. SSE required when a visual is attached. WS only if advertised; same envelopes.

### 2. `/reward`

GET/POST + nullable `reward` + `202` + `/evaluations/:id/events` is enough for GEPA cells, SFT campaigns, and Craftax leaderboards.

v1 = scalar + `node_results[]`. Do not require vector `components`. Campaign aggregates are optimizer-side (split_role + those scalars), not a Containers vector.

Leaderboards / Pareto / SFT tables bind **this field**. Missing stays empty; do not drop the child (C7-O04).

### 3. Child evals

`rollout_id` + stream id + `/reward` URL is the ref. No signed `child_eval_ref`.

```json
{
  "kind": "container_rollout",
  "id": "rollout_…",
  "role": "candidate_evaluation",
  "attributes": {
    "stream_id": "…",
    "reward_url": "/reward?rollout_id=rollout_…"
  }
}
```

Create-rollout must return those three so the sidecar cannot guess. C7-O01 already says this; echo it on the create response, not only in a later GET.

### 4. Connect-before-start

Order is locked: subscribe + ready **before** `POST /rollouts` (C1-08).

HTTP 200 on subscribe is necessary and **not sufficient**.

Wanted: first SSE/poll item is a **transport-control** record that does **not** consume domain sequence:

```text
type: stream.subscribed
stream.id
rollout_id | run_id
next_sequence
ready: true
```

Workshop (TS-E01) treats receipt of that record as ready, then starts. No bidirectional `consumer.ready` POST unless WS is bound. Headless C1-08: client records `stream.subscribed` then POSTs.

First **semantic** event remains `trace.opened`. Heartbeats never count as ready.

### 5. Provisional env-sum

Yes. Craftax live plots RewardSignals from the log (step delta / cumulative as present). `/reward` is the terminal leaderboard cell (`mode=terminal` after seal). Call `mode=provisional` only if `live_reward` is advertised. Harbor never.

### 6. Multiplex

C7-O02 is the container multiplex we need (distinct logs, distinct `/reward`, flip SSE without stall).

Extra: **typed occupancy**. If `scale_leases` cannot admit another episode, create-rollout returns busy/queued — do not interleave into another run’s log. Usage stays keyed by `rollout_id` (C7-O05). Policy pin per child (C7-O03).

No new auth scheme for two local GEPA runs. `optimizer_run_id` isolation is Workshop/sidecar. Containers isolates rollouts and leases.

A4 honest `queued` on a single accelerator is **optimizers-beta**, not Containers — unless the child evals also contend for Craftax leases, in which case the typed busy above is the signal.

### 7. Reopen

Workshop persist-raw + Trace V5 is the reopen path after compute is gone (C7-W02, C7-W06, TS-E02, TS-E06).

**Also need TS-D01 advertised:** frames/artifacts fetchable by digest after `world_stop` for a declared TTL, **or** receipt says `retention: run` so we copy into Desktop CAS during the live window. Silent 404 after stop is a rewrite for Craftax scrub.

Harbor leftover tarball stays the provided-evidence path (C2-10). Stopping a sidecar must not delete mirrored events or visuals.

### 8. Policy pin

Confirmed. Register `luna_med` / `sol_med` via `POST /policy-configs` (C3-05). They are configs, not tasks.

| Role | `policy_ref` |
| --- | --- |
| Craftax 10× player | `{ harness: react, config: luna_med }` |
| Harbor trial agent | `{ harness: harbor_fused, config: codex \| luna }` — two configs **before** start; mid-trial bind refused (C5-02) |
| GEPA proposer | `{ harness: gepa_proposer, config: luna_med \| sol_med }` |
| GEPA child eval | `{ harness: banking77_eval, config\|code: candidate }` — second pin, new rollout |

In-flight attempt keeps the old pin (C7-O03). τ² `user_policy_ref` is the same shape; not in this Desktop cut.

### 9. Anything else that would force a rewrite

If §12 ships **without** the five freezes in the verdict, Desktop will guess URLs or miss ready. If it ships an **outer** `synth.stream-event.v1` wrapper as required, every template breaks.

Do **not** add: `harness_ref`, signed child refs, `components` as required, snapshot-diff SSE, `telemetry.transport=auto` on visual runs, Harbor `live_frames`, Echo restore, missing→0.

C7-W04 kinds are correct: Craftax has frames + RewardSignals, no `reward.txt`; Harbor has trial/verifier, no map. Same reducer.

**A8 extra (C8, not a C7 rewrite):** `mcp_bind` native on the agentic policy and **refused** on basic; `live_frames` / `true_checkpoint` unsupported; `get_session` = reconnect not restore; `DIGBENCH_API_TOKEN` never in the log (TS-A08 / C7-W05). Ready-ACK is before `start_session`. `/reward` is env status (`completed`→1, `game_over`→0, incomplete→null). No provisional.

---

## What is true in tree today (so the plan is not fiction)

| Surface | Today | This cut |
| --- | --- | --- |
| Craftax 10× Luna med | Real via evals gold HTTP; sealed Trace V5 | Same job through Containers HTTP + visual before first Luna call |
| Stream | Snapshot-diff SSE; poll rejected; guessed `/events` vs per-rollout `/stream` | C1 durable log; bind declared id |
| `live.harbor_eval.v1` | Slot **`jobs`**; private-named template already removed | Slot **`stream`**; trial/verifier kinds only |
| `live.container_rollouts.v1` / Craftax prototype | Slot `stream`; local HTML against real SSE | Graduate prototype to versioned template on Containers stream |
| `optimizer.run.v1` | Generic overlay (GEPA/GELO/SFT) | Fallback only; algorithm families are primary |
| GEPA ingest | Tails JSONL every 750 ms; `append_events` writes `optimizer.run.updated` to SQLite and **does not emit** it on `runtime:event` | Bus publish; live visual updates |
| Optimizer sidecar | Allowlisted `synth-optimizers gepa run` child process | `OptimizerManager` ≈ `LagunaManager` |
| SFT | `SftBackend` + `sft_compat` in optimizers; Desktop local smokes; `goex.sft.v1` in beta | `algorithm_id: "sft"` hosted jobs; campaigns = Containers rollouts |
| Specta | 120 commands; export still disabled | Ignore |
| dig.bench | Not in Desktop | `live.digbench.v1` after C8; content, not a fold |

---

## Template inventory (this cut)

One core reducer. Kinds differ. Missing stays missing. Bind declared `stream` (eval) or optimizer run + child resource refs.

| Acceptance | Template | Binds | Must not |
| --- | --- | --- | --- |
| A1 | `live.craftax.v1` (graduate prototype; or keep `live.container_rollouts.v1` if C7-W04 kinds fit) | Craftax stream: NEV, frames, RewardSignals, policy spans | Invent map; fill reward 0; bind slot `live` |
| A2 | `live.harbor_eval.v1` | Harbor stream: planned → launched → tools/stdout → verifier | Fake Craftax map; `live_frames`; slot `jobs` |
| A2 nested DEO | Harbor visual **plus** child Craftax visual | Parent stream + child stream id | Parent advertising `live_frames` |
| A8 | `live.digbench.v1` | Text obs, legal actions, lives/level/steps, status. Same reducer. | Fake dungeon; `live_frames`; Harbor wrap; token in log |
| A3 | `optimizer.gepa.live.v1` | `optimizer_event.v1` | Env frames on optimizer stream |
| A3 | `optimizer.gepa.frontier.v1` | Pareto / incumbent slices | Merged Luna-vs-Sol overlay |
| A3 | `optimizer.gepa.candidate.v1` | Lineage / prompt diff | |
| A3 | `optimizer.gepa.evaluations.v1` | Child `resource-ref` → Containers stream + `/reward` | Flatten NEV into optimizer events |
| A4 / A6 | `optimizer.sft.live.v1` | Training curves, job status, live campaigns | Parallel-array point clouds |
| A4 / A6 | `optimizer.sft.checkpoints.v1` | Checkpoint rail, promotion ≠ ready | |
| A6 | `optimizer.sft.rollouts.v1` | Campaign child rollout refs | Sparse metric arrays as lanes |
| A6 | `optimizer.sft.examples.v1` | Paired baseline vs ckpt | |
| A6 | `optimizer.sft.dataset.v1` | Split roles, `dataset_digest` | |
| A6 | `optimizer.sft.lineage.v1` | Base → adapter → deployable | |
| fallback | `optimizer.run.v1` | Unknown algorithm | Primary GEPA/SFT UX |

`craftax.rollout_scrub.v1` / `craftax.eval_matrix.v1` remain **post-seal** Trace V5 views, not the live connect-before-start path.

---

## Implementation sequence

Work against C7 fixtures/mocks in parallel. Do not invent a second stream. Desktop A1 waits on C3+C7-W receipt. A3/A4/A6 pin C7-O.

```text
  Containers (their cut)
    C0–C2 + C7 floor, then C3 engine / C5 Harbor / C6 Echo
    five freezes above in the receipt
    nightly: C3 --paid

  Workshop W0  (now, mocks OK)
    slot stream everywhere (rename harbor jobs)
    bind declared stream ids only
    persist-raw before publish
    reducer: missing stays missing
    ready gate: no POST /rollouts until stream.subscribed
    emit optimizer.run.updated on runtime:event  (bugfix)

  Workshop W1  (C7-W + C3/C5 receipts)
    live.craftax.v1 from prototype
    live.harbor_eval.v1 trial/verifier
    TS-E01…E08 on real streams
    A1 Desktop, A2 in-app register

  Workshop A8  (C8 receipt; after W1; independent of OptimizerManager)
    live.digbench.v1 — same reducer, new kinds
    visual ready before start_session
    two policy_refs: basic ReAct vs agentic Codex + mcp_bind
    token never in envelopes; reopen after their session is gone
    Capstone. Not a Playwright click-through of digbench.ai.

  Workshop W2
    OptimizerManager next to LagunaManager
    install / start / stop / version
    uninstall ≠ delete mirrored events, visuals, templates
    one spool per run; sidecar_version × algorithm_version × recipe_version

  Optimizers G1  (pin C7-O)
    fail missing sequence; never default usage/reward to 0
    child evals = create-rollout + resource-ref
    two workers (not singleton); Luna vs Sol proposer policy_refs
    A3 Banking77 flip

  optimizers-beta S1
    register algorithm_id: "sft" (not goex.sft.v1)
    two hosted jobs, distinct dataset_digest; multiplex or honest queued
    A4

  optimizers-beta S2
    one multi-checkpoint job; visual open before training
    campaigns = sets of Containers rollouts
    aligned metric records; promotion ≠ checkpoint ready
    reopen after provider gone
    A6
```

### Workshop W0–W2 detail

**W0 bind + honesty**

- Eval driver / Visuals MCP: allocate run + rollout ids → bind visual to declared stream id → wait ready → start.
- Kill constructed URLs in the Craftax gate (`live` slot, container `/events`).
- Persist envelopes to Desktop CAS before renderer publish. Reopen from CAS + Trace V5 with engine gone.
- Cost/usage/reward widgets: `null` → unknown, never `$0.00`.

**W1 templates**

- Same reducer as C7-W04. Harbor fixture has no frames; Craftax fixture has no `reward.txt`.
- Scrub cutoff shows only events ≤ cursor (TS-E03). Seal does not change `visual_id` (TS-E06).
- Do not leak collector capabilities (TS-E05) or private runner names.

**W2 OptimizerManager**

Mirror `LagunaManager`: discovery, digest/signature, version pin, process lifecycle, health, loopback auth, recovery. Sidecar owns compute and authoritative `optimizer_event.v1`. Existing `OptimizerService` stays the durable projection / index / visual integration.

`recipes.rs` today discards the `AppEvent` from `append_events`. Publish it on `runtime:event` or the live GEPA visual never updates (known bug).

Stopping/uninstalling a sidecar version does not delete runs, events, visuals, or retained template packages.

### Optimizers (`algorithm_id: "gepa"`)

- One spool per `optimizer_run_id`. Dual-write `optimizer_event.v1` (already started in `synth_optimizer_platform`).
- Child evals are Containers rollouts. Optimizer events carry search/lineage/budget and **links**, not NEV/frames.
- Two concurrent runs (Luna vs Sol proposer). Flip visual; other log must not stall (sits on C7-O02).
- Fail closed on missing sequence. Missing usage/reward stay null.
- Banking77 is the A3 task. Fake JSONL smokes fail.

### optimizers-beta (`algorithm_id: "sft"`)

- Dedicated optimizer, not `goex.sft.v1`. Same visual primitives, different state machine. `SftBackend` / `sft_compat` in optimizers stay the adapter over `optimizer_event.v1`, not a second DB.
- A4: two hosted Tinker or OpenAI FT jobs, different `dataset_digest`s. If one accelerator: second is honestly `queued`, then starts without corrupting the first log.
- A6: one multi-checkpoint job. Visual connected before training starts. Checkpoint eval campaigns allocate stable `rollout_id`s via Containers. Metrics aligned on `(checkpoint_id, split_role, step, evaluator_version)` — no sparse parallel arrays. Promotion is a decision event, not “checkpoint ready.”
- Reopen after provider/slots are gone from persisted optimizer log + child Trace V5.

---

## Acceptance mapping

| ID | First proof | Then |
| --- | --- | --- |
| A1 Craftax 10× | Containers C3 + C7-W (engine in PR; `--paid` nightly) | W1: visual ready before first Luna call; scrub; seal; reopen |
| A2 Harbor GameBench | Containers C5 | W1: register in-app; `live.harbor_eval.v1` first; two policies |
| A3 two GEPA | C7-O + G1 | W2+G1: real Banking77, two runs, flip |
| A4 two SFT | S1 hosted jobs | W2+S1: not smokes |
| A5 streams | Containers C1 | All consumers: bound transports only |
| A6 SFT ckpt-eval | S2 | W1 reducer + SFT family; campaigns; aligned metrics |
| A7 Echo | Containers C6 | Workshop: do not show as a fold; no restore UI |
| A8 dig.bench | Containers C8 | W1 reducer + `live.digbench.v1` first; two harnesses; no frames; reopen |

Out of cut: Prime GSM8K, Chess OpenEnv, MAPO/RLVR/OHCO, merged Luna-vs-Sol Pareto overlay, two-board layout, Specta exporter, private GameBench/CardBench extensions, GEPA/SFT **on** dig.bench.

---

## Known bugs — do not reintroduce

| Bug | Rule |
| --- | --- |
| Snapshot-diff SSE / local counter (`http_adapter.py`) | Do not build a visual on it. C1-05 fails that server. |
| Slot `live` vs `stream`; guessed URLs | C1-09 / C7-W01. Also rename Harbor `jobs`. |
| `checkpointable=True` on OpenEnv | No restore in UI. |
| `reward or 0.0` / `_clamp_score` | Visuals stay empty. |
| tau3 `breakdown.get(basis, 1.0)` | Missing combiner basis → null. |
| GEPA ingest without `optimizer.run.updated` on the bus | W0: emit on `runtime:event`. |

---

## What we will implement against mocks before the receipt

W0 only: slot rename, reducer honesty, persist-raw, ready gate, bus publish. Fixtures must match C7-W04 kinds and C7-O01 refs.

We will **not** ship A1–A4/A6 as passed on JSONL smokes.

---

## Addendum: A8 dig.bench (containers follow-up)

**C7 and C8 signed off 2026-08-12.** Start the floor.

A8 is the **joint final**. W3 = `live.digbench.v1`. Locked:

```text
  relay over api.digbench.ai (not Harbor, not OpenEnv)
  seven kinds: session.opened, observation, legal_actions, stats, action,
               invalid_action, status
  optional raw JSON on observation / session.opened; no second state event
  agentic MCP on the same eval/trace stream (policy spans)
  live_frames / true_checkpoint unsupported
  get_session = reconnect
  /reward: completed→1, game_over→0, else null
  two policy_refs: basic (mcp unused) vs agentic (mcp_bind)
  stream.subscribed before start_session (C1-08)
  one public game frozen on the receipt (P-1 or first list_games)
  PR mock may skip agentic MCP; nightly digbench_public must not
  A8 Desktop needs both harnesses
  token never in the log
```

Do not: fake a dungeon, wrap their HTTP in Harbor, treat Playwright on digbench.ai as A8, GEPA-on-dig.bench this cut.
