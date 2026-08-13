# Handoff: Optimizers + Workshop visuals (Aug 12)

**For:** whoever is planning the optimizer sidecar / Workshop live-visual work  
**From:** containers + eval-platform design thread (2026-08-12)  
**Ask:** Read this, plan Optimizers + Workshop (and optimizers-beta SFT) against the contracts below, then tell us **what else Containers must freeze** before we start the containers version.

**Signed off 2026-08-12.** C7 and C8 are enough. **Start the floor.**

This is not a replacement for the notes. It is the index and the split of work.

**Reply (2026-08-12):** [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md). **§12 C7 is enough** with five small freezes (stream descriptor on create-rollout, `stream.subscribed` control record, typed occupancy, advertised artifact retention, consumer cursor = sequence). No second stream. No outer envelope. No signed `child_eval_ref`. Floor may start.

**Follow-up (same day):** A8 [dig.bench](https://digbench.ai) is the **Workshop capstone**. **C7 and C8 are both signed off.** Floor may start. Patches from that sign-off are in `container_compat.md` §4.7 / §12 (sequence cursor, `stream.subscribed` on C1-08, full stream descriptor, fail slot `jobs`, typed occupancy, advertised retention, no `auto` on authoritative runs).

---

## Read first (in this order)

| Doc | Why |
| --- | --- |
| [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md) | **Reply.** Workshop + Optimizers plan, C7 answers, five freezes. |
| [`aug_12_update.md`](./aug_12_update.md) | Parent note. Acceptance **A1–A8**. Stream/visual/optimizer product rules. Implementation cut. |
| [`container_compat.md`](./container_compat.md) | What Containers will actually ship. Nouns, `/reward`, affordances, ASCII **§Map**, per-bench honesty, **§12** programmatic suite. |
| [`live_evals.md`](./live_evals.md) | Trace Streaming Profile. **TS-A…E** (especially TS-C transport, TS-E Workshop consumer). Connect-before-start. |
| [`live_optimizers_gepa.md`](./live_optimizers_gepa.md) | Optimizer profile, `optimizer_event.v1`, GEPA template family, child-eval drill-down, sidecar lifecycle, SFT contract. |
| [`execution_stream_contracts.md`](./execution_stream_contracts.md) | Durable log, poll/SSE/WS, missing ≠ 0, provider discovery. |
| [`execution_ontology.md`](./execution_ontology.md) | Shared nouns. Treat **policy = harness + config** as the Aug 12 correction (see below); do not plan a sibling `HarnessService`. |

Authority for “is this a fold?” remains the master plan. Harbor is the only first-class external fold. GameBench / τ-bench / **dig.bench** are **content**.

**Do not use** `aug_12_notes.md` (unsaved dump). **Do not** wait on Specta codegen.

---

## What we are trying to make true

Today’s 10-lane Luna med Craftax is real (CLI → gold HTTP → Trace V5). It does **not** go through `synth-containers` HTTP, a durable poll/SSE/WS contract, or a Workshop visual. That is the same job CUA keeps failing in-app.

The dogfood prompt is still:

> Find the Craftax Rust GameBench container, run exactly 10 rollouts, collect Trace V5 / rewards, open a visual that compares them.

Plus: two live GEPA runs (Luna vs Sol proposer) you can flip between; two standalone SFT jobs; Harbor GameBench with `live.harbor_eval.v1` open **before** start.

**Capstone (A8):** a public [dig.bench](https://digbench.ai) game through Containers, `live.digbench.v1` open first, two harnesses (basic vs agentic MCP), text evidence only, reopen after their session is gone. If that needs a second stream or a Harbor wrap, A1–A7 were not done.

---

## Split of ownership

```text
  Containers version (start the floor)
      pins, affordances, durable eval/trace log, POST/GET /reward
      Craftax + Harbor + Echo + DEO HTTP + dig.bench relay (C8 mock)
      §12 suite + receipt  synth.container-compat-conformance.v1
      signed-off freezes: sequence cursor, stream.subscribed, full descriptor,
      fail slot jobs, typed occupancy, advertised retention, no auto

  You (plan now; implement after that receipt, or in parallel on mocks
  that match §12 C7 — do not invent a second stream)

      Workshop     visuals, persist-before-publish, connect-before-start,
                   bind declared stream IDs, OptimizerManager ≈ LagunaManager,
                   reopen after compute is gone

      Optimizers   algorithm_id: "gepa", optimizer_event.v1, multiplex,
                   child evals = resource refs into Containers

      optimizers-beta   algorithm_id: "sft" (not goex.sft.v1), hosted jobs,
                   checkpoint campaigns → child Containers rollouts
```

A1 / A2 / A5 / A7 / **A8 headless (C8)** are **Containers-first**, then you make them true in Desktop.  
A3 / A4 / A6 are **yours**, but they **pin** the Containers receipt (especially §12 **C7-O**).  
A8 Desktop (`live.digbench.v1`) is **yours** after C8. It is the capstone for the whole update.

---

## Locked decisions you should not reopen

From `aug_12_update.md` §1 and `container_compat.md`:

1. **Harbor** is the only first-class external fold. OpenEnv / Prime are wraps. Archipelago is research. TB3 / TBLite are Harbor **datasets**.
2. **Policy = harness + config** (+ optional code). No sibling `HarnessService`, no `harness_ref` on create-rollout. `PUT /policy { harness | config | code }`. `restart_policy` bounces the loop. Harbor’s fused agent is this shape.
3. **Container ≠ environment.** World is heavy; task is light.
4. **`POST /reward`** produces the attempt reward (`container_compat.md` §3). `GET` before POST → `reward=null`, not `0`. Env `RewardSignal`s stay in the log; `/reward` does not step. Harbor `reward.txt` is a **script node**, not env reward.
5. **One durable log per advertised stream.** Request names `poll` / `sse` / `websocket`; server returns what it bound; **no silent degrade**. Poll is required. Heartbeats do not advance cursors. EOF is not completeness.
6. **Missing ≠ 0** for reward, score, usage, sequence.
7. **Connect-before-start. Persist-before-publish.** Visual ready before the first paid/mutating event. Slot name is **`stream`**, not `live`. Bind **declared stream IDs**, never construct `/events` vs `/rollouts/{id}/stream`.
8. **Three evidence layers:** raw capture → live projection (`synth.trace-stream-event.v1`) → sealed Trace V5. ATIF is a projection, not the log.
9. **`optimizer_event.v1`** is the optimizer envelope. Child evals stay eval/trace streams + `/reward`. Do not flatten Craftax NEV into optimizer events.
10. **`goex.sft.v1` ≠ `algorithm_id: "sft"`.** Same visual primitives, different state machines.
11. Flip between two live optimizer visuals is enough; no side-by-side two-board requirement this cut.
12. Private Evals runner names stay out of public templates.
13. **dig.bench is content** (hosted env + relay). Not a fold. Not Harbor. Not OpenEnv. A8 is the Workshop capstone on the same contracts.

`aug_12_update.md` open question 10 (“HarnessService as the shared recipe”) is **superseded**: the recipe is a `policy_ref` (ReAct harness + Luna med config).

---

## What Containers will freeze (so you can plan against it)

Normative detail: `container_compat.md` **§Map**, **§3 `/reward`**, **§12**.

### Pins on create-rollout

```text
world_ref  environment_ref  policy_ref  user_policy_ref?
task_ref  evaluation_plan_ref  task_instance_id  stream
```

`policy_ref` = `{ harness, config, code? }`. τ² also has `user_policy_ref` (second policy, same shape).

### Streams you will bind

| Kind | Template / consumer | What is in the log |
| --- | --- | --- |
| Craftax interactive | live Craftax visual (graduate local HTML) | NEV, frames, RewardSignals, policy spans |
| Harbor trial | `live.harbor_eval.v1` | planned → launched → tools/stdout → verifier. **No fake map.** |
| Nested DEO | Harbor visual **plus** child Craftax visual | Child has frames; parent must not advertise `live_frames` |
| dig.bench (A8) | `live.digbench.v1` | observation, legal_actions, stats (level/lives/steps), action, status. **No frames.** |
| GEPA child eval | `optimizer.gepa.evaluations.v1` drill-down | Containers rollout stream + `/reward`, linked by `rollout_id` |
| SFT checkpoint eval | `optimizer.sft.rollouts.v1` (see live_optimizers) | Same: child rollout refs, not parallel metric arrays |

Headless proof Containers will run **without Desktop**: §12 **C1** (TS-C) + **C7-W** (subscribe declared id, persist, replay with engine gone, missing stays null, Craftax vs Harbor kinds on one reducer).

### `/reward` you will display

```text
GET  /reward?rollout_id=     absent → status=absent, reward=null
POST /reward                 compute; idempotent unless rescore
GET  /evaluations/:id/events long Harbor verifier (202)
```

Leaderboards / Pareto / SFT campaign tables use **this field**, not a hole-filled step array. Craftax A1: env-sum of log RewardSignals. Harbor: script node. DEO parent: held-out **gate**, not a copy of child env-sum. dig.bench: env `completed`→1 / `game_over`→0; lives/level are stats. `container_compat.md` §3 / §4.11.

### Affordances (bind fail-closed)

Workshop/optimizer recipes declare `require | prefer | unused`. Harbor TB3 + `require true_checkpoint` **refuses**. Echo does not claim restore. Craftax `live_reward` (provisional env-sum) may be native; Harbor and dig.bench provisional is unsupported. dig.bench + `require live_frames` **refuses**.

### Multiplex (A3/A4 at the container layer)

§12 **C7-O**: two concurrent child rollouts, distinct logs, distinct `/reward`, flipping which SSE client you read does not stall the other. That is the substrate for two GEPA runs / two SFT jobs. Sidecar must not serialize behind a singleton worker **on top of** that.

---

## What you own (plan these)

### Workshop

`aug_12_update.md` §2.4, §3 Workshop; `live_evals.md` TS-E01…E08; `container_compat.md` §12 C7-W.

- Visual connected and **ready** before `POST /rollouts` / first paid call (A1, A2, A3, A6).
- Bind discovery’s stream id. Kill the Craftax slot mismatch (`live` vs `stream`).
- Persist-before-publish: raw envelopes on disk; reopen after container/sidecar gone.
- Templates: Craftax live (frames + env reward series); `live.harbor_eval.v1` (trial/verifier); **`live.digbench.v1`** (text obs/actions/stats — W3 / A8); GEPA family from `live_optimizers_gepa.md`; SFT family (`optimizer.sft.{live,checkpoints,rollouts,examples,dataset,lineage}.v1`). Same core reducer; kinds differ.
- Missing reward/usage render as unknown, never `$0.00` / `0`.
- OptimizerManager next to LagunaManager. Stopping a sidecar must not delete mirrored events or visuals.
- Do not leak private Evals names. Do not block on Specta.
- A8: visual ready **before** `start_session`. Do not Harbor-wrap. Do not draw a dungeon. Token never in the log. Two `policy_ref`s (basic vs agentic). Playwright against digbench.ai itself is not A8.

A1 in Desktop is TS-E01 on top of Containers C3. Fake JSONL smokes do not pass.

### Optimizers (`algorithm_id: "gepa"`)

`live_optimizers_gepa.md`; A3.

- Two runs at once (Luna vs Sol proposer), distinct `optimizer_run_id`, logs, budgets, Pareto, visuals.
- Child evals = Containers `rollout_id` + stream + `/reward` (C7-O01). `optimizer_event.v1` carries search/lineage/budget and **links**, not env frames.
- Fail missing sequence; never default usage/reward to 0.
- One spool per run. Flip visual without stalling the other.

### optimizers-beta (`algorithm_id: "sft"`)

`aug_12_update.md` A4 + A6; `live_optimizers_gepa.md` SFT section.

- A4: two hosted jobs, different `dataset_digest`s, multiplex or honest `queued`. Not `goex.sft.v1`. Not two fake Craftax JSONL smokes.
- A6: one multi-checkpoint job, visual **open before training**. Aligned metrics (no sparse parallel-array point clouds). Checkpoint eval campaigns = sets of Containers rollouts. Promotion ≠ “checkpoint ready.” Reopen after provider/slots are gone.

Implementation spec: [`optimizers_beta_sft.md`](./optimizers_beta_sft.md). Recipe `sft.craftax.nemotron-nano.tinker.v1`.

---

## Acceptance cheat sheet

| ID | Who proves it first | Your job after Containers receipt |
| --- | --- | --- |
| A1 Craftax 10× | Containers C3 + C7-W (engine in PR; `--paid` nightly) | Desktop visual before first Luna call; scrub; seal; reopen |
| A2 Harbor GameBench | Containers C5 | Register task in-app; `live.harbor_eval.v1` first; two policies |
| A3 two GEPA | You (pin C7-O) | Real Banking77, two sidecars/runs, flip |
| A4 two SFT | You | Hosted jobs, not smokes |
| A5 streams | Containers C1 | Consume bound transports only |
| A6 SFT ckpt-eval | You | One job, campaigns, aligned metrics |
| A7 Echo | Containers C6 | Do not show Echo as a first-class fold |
| A8 dig.bench | Containers C8 (mock PR; `--paid` nightly) | W3: `live.digbench.v1` first; two harnesses; no frames; reopen |

Out of cut (do not plan into this): Prime GSM8K, Chess OpenEnv, MAPO/RLVR/OHCO, merged Luna-vs-Sol Pareto overlay, two-board layout, GEPA/SFT **on** dig.bench, private dig.bench tiers.

---

## Known bugs you must not reintroduce

| Bug | Where it lives | Your rule |
| --- | --- | --- |
| Snapshot-diff SSE / local counter | containers `http_adapter.py` | Do not build a visual on it; C1-05 fails that server |
| Slot `live` vs `stream`; guessed URLs | Workshop Craftax gate | C1-09 / C7-W01 |
| `checkpointable=True` on OpenEnv | containers `compat/openenv.py` | Do not offer restore in UI |
| `reward or 0.0` / `_clamp_score` | containers rubrics/recovery | Visuals stay empty |
| tau3 `breakdown.get(basis, 1.0)` | evals tau3 container | Product combiner missing → null |
| GEPA ingest without `optimizer.run.updated` on the bus | Workshop today | Live visual never updates |

---

## What we need back from you (before Containers starts)

**Answered 2026-08-12** in [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md). Short form:

1. **Stream bind.** Yes: declared `stream.id` + poll/SSE + `Last-Event-ID` (+ optional WS). Two concrete schemas only (`trace-stream-event.v1`, `optimizer_event.v1`). Trait, not an outer envelope. **Freeze:** stream descriptor on create-rollout response.
2. **`/reward`.** Yes: GET/POST + nullable `reward` + `202`. v1 = scalar + `node_results[]`. Do not require `components`.
3. **Child evals.** `rollout_id` + stream id + `/reward` URL as `synth.resource-ref.v1`. No signed `child_eval_ref`. Echo the three on create-rollout.
4. **Connect-before-start.** C1-08 order is right. HTTP 200 is not enough. Wanted: non-advancing `stream.subscribed` control record (`stream.id`, `next_sequence`). Then `POST /rollouts`. First semantic event stays `trace.opened`.
5. **Provisional env-sum.** Yes. Craftax plots log RewardSignals; `/reward` is the terminal cell. Harbor: unsupported.
6. **Multiplex.** C7-O02 is enough plus **typed occupancy refusal** when leases are exhausted. No new auth scheme. Usage keyed by `rollout_id`.
7. **Reopen.** Persist-raw + Trace V5 is the Desktop path. **Advertise** TS-D01 TTL or `retention: run` so frames are copied live. Silent 404 after `world_stop` is a rewrite.
8. **Policy pin.** Confirmed. `luna_med` / `sol_med` are policy configs. GEPA proposer vs child-eval are two `policy_ref`s. No `harness_ref`.
9. **Rewrite risks.** Outer envelope; missing the five freezes; `auto` transport on visual runs; consumer-facing `nev_cursor`; Harbor `live_frames`; missing→0.

C7-W / C7-O are not wrong for Desktop. Slot `jobs` on `live.harbor_eval.v1` is a Workshop bug we will rename to `stream`; C1-09 should fail `jobs` and `live`.

A8 (`live.digbench.v1`) consumes the same bind/ready/`/reward` contracts. C8 kinds go on the same reducer. Optimizers unused on dig.bench this cut.

**C8 answered 2026-08-12 (same bar as C7):** §12 C8 is enough. Short form:

10. **Kinds.** Those seven are enough. Optional: attach their raw JSON as payload on `observation` / `session.opened`. Do not add a second `state` event the reducer must understand.
11. **MCP timeline.** Same eval/trace stream as Harbor tools/stdout (policy span open/data/close). Not a nested optimizer stream. Not a second log.
12. **Ready vs `start_session`.** C8-06 matches W0. Allocate ids if you want; **mutating/token starts at `start_session`**. Subscribe + `stream.subscribed` before that call.
13. **Pin.** Freeze one public game id on the receipt (tier-1 P-1, or first `list_games` entry if P-1 is gone). Not a 70-game scrape.
14. **Two harnesses.** A8 Desktop pass needs **both** in-app. PR mock may skip agentic MCP; nightly `digbench_public` must run it (C8-11). Basic-only Desktop + agentic-headless does **not** pass A8.

Lift `stream.subscribed` from C8-06 into **C1-08** so Craftax/Harbor get the same control record. Consumer cursor is **sequence** (C7 freeze 5); `nev_cursor` may stay internal. C1-09 / C7-W01 should also fail slot `jobs`.

**Applied** in `container_compat.md` (C0-08/C0-09, C1-01…C1-09, C3-02, C5-03, C7-W01, C8, §4.7). **Start the floor.**

---

## Suggested planning outline (yours to change)

```text
  1. Template inventory vs A1/A2/A3/A4/A6
     Craftax live, live.harbor_eval.v1, gepa.{live,frontier,candidate,evaluations},
     sft.{live,checkpoints,rollouts,...}
  2. Bindings: stream id, cursor, /reward, child_eval refs, visual_id identity across seal
  3. OptimizerManager lifecycle (mirror LagunaManager); persist; uninstall ≠ delete
  4. Reducer: one core; kinds per template; missing stays missing
  5. A3/A4 multiplex on top of C7-O; A6 campaign state machine
  6. Gap list → Containers (this handoff § “what we need back”)
```

**Done for C7 and C8.** Plan is [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md). **Start the floor.**
