# Aug 12 update — Containers + Optimizers

**Status:** Single working note for the systematic update. Not a replacement for the master plan.

**Sources:** `execution_platform_master_plan.md`, `execution_ontology.md`, `execution_stream_contracts.md`, `live_evals.md` (Trace Streaming Profile + TS-A…E), `live_optimizers_gepa.md` (SFT contract), `private_eval_workspace_extensions.md`, plus the 2026-08-11/12 Workshop refactor, CUA, and design-thread context.

**Empirical slice (today):** 10-lane Luna medium Craftax against native GameBench rust via `evals/suites/nonproduct/craftax`, artifacts in `containers/temp/_runs/luna_med_10x/`. Mean reward +3.31, 0 failed lanes, Trace V5 sealed. The loop is real. It does **not** go through `synth-containers` HTTP, Workshop visuals, or a durable poll/SSE/WS contract. That is the same 10-rollout + visual job Workshop agents keep failing in-app.

**First workstream:** `container_compat.md` — env / policy / world / task, `/reward`, why LCD Harbor+OpenEnv+Prime+Archipelago compat fails, Harbor fold first. Containers-first programmatic suite: **§12** (ship that version before evals/optimizers/workshop move).

**Handoff for Optimizers + Workshop visuals:** [`HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./HANDOFF_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md). They plan A3/A4/A6 and Desktop TS-E against §12 C7, then tell us if Containers needs anything else before that cut starts.

**Capstone:** [dig.bench](https://digbench.ai) is **A8** — a full Workshop run with `live.digbench.v1` open first. Content, not a fold. Nouns in `container_compat.md` §4.11 / §Map. This is the final acceptance for the whole update, not a replacement for A1–A7.

---

## Acceptance tests

Pass means a real run in Workshop (or a recorded hosted job Workshop can reopen), connect-before-start, persist-before-publish, no invented fields, no private Evals runner names on public surfaces. Fake JSONL smokes do not pass.

**Result 2026-08-12 evening — receipts in [`receipts/2026-08-12/`](./receipts/2026-08-12/README.md):**

| ID | Result | One line |
| --- | --- | --- |
| **A1** | **PASS** | 10/10 paid Luna lanes through Containers HTTP, visual bound before every first paid call, $0.0311 |
| **A2** | **not done** | Harbor Docker bundle runs agent+verifier as distinct executions, but `harbor_docker.py` has no pinned-bundle path, so nothing reached Workshop |
| **A3** | **PASS** | two live Banking77 GEPA (Luna vs Sol), disjoint logs, four flips, no stall |
| **A4** | **PASS** | two hosted Tinker SFT completed in parallel, distinct dataset digests, `costUsd: null` |
| **A5** | **PASS** | poll ≡ SSE ≡ WS on the paid A1 stream; reopens after the container is gone |
| **A6** | **partial** | 7/7 structure checks; campaign rollouts score `null` — the container cannot sample a Tinker checkpoint locally |
| **A7** | out of cut | — |
| **A8** | **blocked** | `api.digbench.ai` 401; no `DIGBENCH_API_TOKEN` on this machine |

Nine defects were found by running these and fixed in place; they are listed in
the receipt index. Nothing is committed.

| ID | Test | Pass when |
| --- | --- | --- |
| **A1** | **Craftax Luna med 10×** | Same job as today’s CLI slice, launched through Containers with `environment_ref` / `policy_ref` (harness + config) / `task_world` (seeds 0–9) and an explicit stream bind. Workshop visual connected before the first paid call. Poll / SSE (/ WS) equivalent. Spans open/data/close. `capture.closed` then a reconciled Trace V5. This is `CRAFTAX-LUNA-010` / TS-E01. **Containers-first:** `container_compat.md` §12 C3+C7 (engine path in PR CI; `--paid` react on nightly). |
| **A2** | **Harbor GameBench live** | User registers a Harbor-packaged GameBench task (not a GameBench wire format). Pins at least two policies (e.g. Codex vs Luna). Opens `live.harbor_eval.v1` first. Starts; streams trial/attempt evidence; seals Trace V5. Native vs wrapped verifier agrees. ATIF is a projection, not the log. |
| **A3** | **Two GEPA instances in parallel** | Banking77, two `algorithm_id: "gepa"` runs at once. Run A proposer = Luna, run B proposer = Sol. Distinct `optimizer_run_id`s, event logs, budgets, Pareto fronts, visuals. Both stay live; flipping the open visual does not stall the other. No crossed usage. Sidecar multiplexes; do not serialize behind a singleton worker. |
| **A4** | **Two standalone SFT instances in parallel** | Two `algorithm_id: "sft"` runs (optimizers-beta dedicated optimizer, **not** `goex.sft.v1`). Different `dataset_digest`s. Distinct run IDs, logs, checkpoints, visuals. Both in flight, or the second is honestly `queued` on a single accelerator and then starts without corrupting the first log. Two fake Craftax SFT smokes writing JSONL fail this test. Hosted Tinker jobs required. The shut-down OpenAI Fine-tuning API is not a substitute. |
| **A5** | **Durable stream contract** | One append-only log per advertised stream. Request names `poll` / `sse` / `websocket`; server returns what it bound; no silent degrade. Cursor on `GET .../events`. Heartbeats do not advance evidence. Missing sequence / reward / usage / score fail closed (never default to 0). TS-C01…C08 on a reference server. |
| **A6** | **SFT checkpoint-eval (one job)** | Separate from A4. One hosted multi-checkpoint `sft` run opened before training. Aligned metrics (no parallel-array point clouds). Concurrent checkpoint evaluation campaigns with stable rollout IDs, split roles, promotion ≠ “checkpoint ready.” Reopen after provider/slots are gone. |
| **A7** | **OpenEnv Echo wrap** | Unmodified Echo image. Native-vs-wrapped fixed actions. Drop the false `checkpointable=True` snapshot claim. Compatibility target only; not a first-class fold. |
| **A8** | **dig.bench Workshop capstone** | Public game from [digbench.ai](https://digbench.ai) launched through Containers. Visual `live.digbench.v1` connected **before** `start_session`. Two `policy_ref`s on the same game: **basic** (ReAct / next-action) and **agentic** (Codex or equivalent + `digbench-mcp`). Stream observation, legal actions, lives/level/steps, history. No fake frames. `/reward` from env `status` (`completed` / `game_over`); incomplete stays `null`. Seal Trace V5. Reopen after the relay and their session are gone. Token never in the log. **This is the final acceptance for A1–A7 contracts on a third content family** (hosted remote text env + MCP bind + two harnesses). Fake JSONL or a Harbor wrap of their HTTP API fails. |

**Out of this cut:** Prime GSM8K, Chess OpenEnv, private GameBench/CardBench extensions, MAPO/RLVR/OHCO, a merged Luna-vs-Sol Pareto overlay, a side-by-side two-board layout (flip is enough), GEPA/SFT **on** dig.bench, private (non-public) dig.bench tiers.

A3 and A4 are the same **multiplex** proof on two algorithms. A6 is SFT training/eval quality inside one run. Do not collapse them.

---

## 0. Product reality this update has to serve

Workshop v0.2 already has the nouns: Chats, Research, Visuals, Optimizers, Data (Containers · Traces · Usage), Inference. Laguna is a first-class sidecar in the UI (resident GB, prefill, in-flight). The dogfood prompt is:

> Find the Craftax Rust GameBench container, run exactly 10 rollouts, collect Trace V5 / rewards, open a visual that compares them.

CUA and friends-release (`CRAFTAX-LUNA-010`) keep hitting the same class of failure: skill/container discovery, guessed stream URLs, connect-after-run, and invented or missing live evidence. The v0.2 refactor landed 120 Specta-annotated commands, Laguna/Whisper on the supervisor drain, and a single `runtime:event` bus — but generated Specta export is still disabled (`i64`/`u64` vs JSON / `serde_json::Value` overflow). Do not block this update on that exporter.

`synth-containers` is an **opinionated executable façade**. It folds supported formats into one task/runtime/rollout/stream surface. It is not a lowest-common-denominator protocol library. Harbor is the only public first-class fold. Private Evals runner names stay inside `evals/`. The public live template is `live.harbor_eval.v1` (the old private-named template was removed).

---

## 1. What is already decided

Do not reopen these unless a promotion gate fails.

| Decision | Implication |
| --- | --- |
| Harbor is the only committed first-class external fold | Own adapter, fixtures, CI, docs, support. Terminal-Bench / TBLite enter as Harbor datasets. Public Workshop names Harbor, never private Evals runners. |
| OpenEnv and Prime Verifiers are compatibility targets | Wrap unmodified packages. Promote only after CA + OE/PV suites. |
| GameBench / CardBench / **dig.bench** are content, not wire formats | Execute through Containers. dig.bench is a **hosted** game server (`api.digbench.ai`), not a Harbor package and not OpenEnv. |
| Private Evals vocabulary stays in Evals | Workshop sees a generic workspace extension + provider. |
| A container is a deployment unit | Environment, Policy, Harness, Evaluator, Artifact, Relay, Optimizer are logical services with independent generations even in one process. |
| Three evidence layers, never one ambiguous format | Raw capture (`synth.capture.raw.v1`) → semantic live projection (`synth.trace-stream-event.v1`) → sealed Trace V5 + reconciliation. |
| Durable log is truth; transports are adapters | Poll, SSE, and WebSocket yield the same ordered IDs and digests. EOF / `[DONE]` is not completeness. |
| OpenResponses discipline, not OpenResponses wire | Items open before deltas and close before parent terminal. Keep resumable SSE IDs. Require `capture.closed` + high-water, then seal. |
| Missing ≠ zero | Reward, score, usage, sequence fail closed. |
| Connect-before-run | Visual ready before paid execution. Persist-before-publish. |
| Shared discovery spine | `synth.execution-provider-info.v1` with discriminated `container` vs `optimizer` profiles. Algorithm/env payloads stay namespaced. |
| `optimizer_event.v1` is the optimizer envelope | Child evals stay eval/trace streams. Do not flatten. |
| `goex.sft.v1` ≠ standalone `algorithm_id: "sft"` | GELO plugin vs hosted SFT optimizer. Same visual primitives, different state machines. |

Master-plan phases 0–8 and CA/OE/PV plus TS-A01…TS-E08 remain the promotion gates.

---

## 2. Four changes this update must land

### 2.1 Standardized streaming: poll / SSE / WebSocket

**Rule:** one durable ordered log per advertised stream. Transports are equivalent delivery adapters over cursors. The **rollout (or run) request names what the caller expects**. Discovery advertises which transports exist; the server **binds** a subset and returns it. Authoritative runs must not silently degrade.

| Transport | When to use | Not for |
| --- | --- | --- |
| **SSE** | Default one-way live tail. Workshop visuals, eval lanes, optimizer fanout. Resume via `Last-Event-ID`. | Bidirectional control. |
| **WebSocket** | Bidirectional flow control, interactive step/reset, OpenEnv-style persistent env sessions. Same envelope as SSE. | A second event schema. |
| **Poll** | Required recovery and compatibility. Headless jobs, NAT/proxies, reconnect backfill, engines that only dump a log (Craftax `GET /rollouts/:id/event_log`). `after_sequence` / `after_ordinal`, `limit`, `high_water`, `terminal`. | A lesser or snapshot-only format. |

Heartbeats are transport control. They must not advance evidence cursors (TS-C04).

**Create-rollout telemetry (Containers):**

```text
telemetry.enabled
telemetry.transport        poll | sse | websocket
                           (required for authoritative / visual-attached runs)
                           auto is refused on those runs (headless smoke only)
telemetry.accepted         full stream descriptor on create-rollout response:
                           stream.id, transports.{poll,sse,websocket}.url,
                           cursor.kind=sequence, reward.url, auth.mode, retention
telemetry.cursor           sequence   (consumer-facing; required)
                           producer_kind? nev_cursor | ordinal  (internal)
telemetry.detail           minimal | standard | debug
telemetry.frame            enabled, format, every_n_steps, digest required
telemetry.retention        run | TTL   (advertised; silent 404 after world_stop fails)
```

`auto` is a convenience for headless smoke only. Authoritative / visual-attached runs **name** `poll` / `sse` / `websocket`. If a visual is attached, the bound transport must be SSE or WS plus poll backfill. HTTP 200 on subscribe is not ready — wait for `stream.subscribed`.

**Containers floor (2026-08-12, in-process PR CI):** poll is required; create-rollout echoes the full stream descriptor (`cursor.kind=sequence`); `GET .../events?after=` is sequence; SSE and WebSocket read the durable log (not snapshot-diff); `telemetry.transport=auto` is refused on authoritative runs; `stream.subscribed` is a non-advancing control record. Suite: `tests/conformance/container_compat/run.py` (C0–C8). Engine ReAct + IsolatedPolicyProcess examples run through that HTTP, not evals gold CLI. Façade is `TargetRuntime` children under `platform/runtimes/`. `deo_nested` parent `/reward` is gate + baseline delta, not a copy of child env-sum. **Not claimed:** paid Luna `craftax_react`, rust gold HTTP, Harbor Docker, live dig.bench token, unmodified Echo image.

**Trace Streaming Profile (Containers owns the kit):** raw envelopes stay immutable; semantic events are a deterministic projection (`trace.opened` → nested `session`/`span` open-data-close → `capture.high_water` / `capture.closed` → `trace.sealing` → `trace.completed` with digest). Representative sequence is in `live_evals.md`. Conformance is TS-A…E (38 tests), not “JSON that looks like events.”

**Optimizer streams** use the same delivery rules on `optimizer_event.v1`. High-frequency env frames stay on the eval/trace stream; the optimizer stream carries campaign lifecycle, aggregates, checkpoint identity, and links. Known bugs: missing sequence → 0, missing usage → 0, MAPO still on `synth_mapo.v1`.

**Workshop:** persist-before-publish. Cursor backfill is reconnect authority. Historical viz must survive sidecar/provider disappearance.

### 2.2 Environment, Policy, and TaskWorld as first-class

Stop using `env: {}` / `policy: {}` / `task_metadata` bags as the semantic model.

| Noun | Owns | Craftax example |
| --- | --- | --- |
| **Environment** | Mutable world, reset/step/tools, env-authored reward/done | GameBench rust gold HTTP |
| **Policy** | Model/provider/effort/tools, session, usage, policy trace | `gpt-5.6-luna:medium` |
| **Harness** | Interaction loop, plan bounds, retries, correlation | `ReActHarness` (plan 5–20, compact-every 16) |
| **TaskWorld** | Initial scenario + instance knobs | `world=craftax_default`, `rules=symbolic_survival`, **seed 0–9** (seed *is* the world instance) |
| **TaskInstance** | Fully resolved pin of the above | workshop `craftax:test:2001`; nonproduct seed+world+rules |

**Required on create-rollout / create-eval:**

```text
environment_ref     service + generation + world_revision
policy_ref          { harness, config, code? }   # no sibling harness_ref
task_world          { world_id, revision, seed?, split?, role?, extras }
task_instance_id    content-addressed resolution
stream              telemetry from §2.1 (declared stream.id; slot stream)
```

Logical service IDs exist even when all four share a process. That is how Workshop binds a visual to the env stream, Laguna to policy, and an optimizer sidecar to child evals without rewriting the task.

Today’s 10-lane run already split these in practice. Containers HTTP now types `world_ref` / `environment_ref` / `policy_ref` `{harness, config, code?}` / `task_instance_id` / declared `stream`. Recipe is that pin — no sibling `HarnessService`.

### 2.3 Popular evals and formats — blockers

“Compatible” = wrap without rewriting task logic; normalized run is faithful; native evidence remains; acceptance suite passes.

| Format / system | Level | Current state | Blockers | Improve? |
| --- | --- | --- | --- | --- |
| **Harbor** + ATIF 1.5–1.7 | First-class fold | Capability labels + resource refs. ATIF import/export with declared loss. Public template `live.harbor_eval.v1`. | No launch/lease/supervision. No native-vs-wrapped verifier suite. Agent vs verifier not distinct executions. Snapshot SSE cannot carry live Harbor evidence. | **Yes — finish this fold.** Trial→Attempt, job→EvaluationRun. ATIF is a projection, not the durable log. |
| **Terminal-Bench / TBLite** | Harbor datasets | None | Inherit Harbor gaps. TBLite calibration is a dataset revision, not a runtime kind. | After Harbor v1, pin public fixtures. |
| **OpenEnv** | Compatibility target | Labels gym-style. **`checkpointable` defaults false; `state()` is not `true_checkpoint`.** | No unmodified-image gateway. WS lifecycle ≠ producer generations. | **Out of this cut.** |
| **Prime Verifiers** | Compatibility target | **No adapter.** | Preserve Taskset/Harness/Env. Metrics ≠ rewards. Local tests must never `prime eval push`. Pin `primeintellect/gsm8k`. | After Harbor + Echo. |
| **Inspect AI** | Research / Echo tutorial | None | Third runner. | **Do not fold.** Echo reference path only. |
| **OpenResponses** | Inspiration | None as a fold | Lifecycle/conformance pattern only. | Adopt discipline in the Trace Streaming Profile. Do not implement OpenResponses as a compatibility layer. |
| **Archipelago / APEX** | Research | HTTP proxy (`synth_http`) | Snapshot + MCP + post-seal grading. | Not in this promotion set. |
| **GameBench rust HTTP** | Native content | Evals talks to gold **directly**. Trace V5 Craftax adapter exists. | Whole-log poll, no `since`. Silent 120-step world default. Cadence markers ≠ checkpoints. Orphan docker ≠ identity. | **Native EnvironmentService + §2.1 relay.** Keep NEV kinds verbatim. Do not invent a GameBench protocol. |
| **evals.event-stream.v1** | Native live eval | Nonproduct uses it. Workshop slots still mismatch (`live` vs `stream`, container `/events` vs per-rollout SSE). | Parallel snapshot `synth.rollout.event.v1`. | Normalize at the boundary onto the Trace Streaming Profile. |
| **Trace V5** | Sealed evidence | Strong (inspect, ATIF, Craftax promote). Today’s run sealed `trace_8cc6b5ba…`. | Live path does not produce V5 incrementally. | Seal/replay authority. Not the live transport. |
| **OpenAI Fine-Tuning API** | Optimizer adapter | Public `SftBackend` + `sft_compat`. Beta: `goex.sft.v1` and standalone SFT path. | Events too thin for checkpoint-rollout viewer. Parallel metric arrays mis-align when sparse. | Adapter over `optimizer_event.v1`, not a second DB. |
| **Gymnasium** | Overlaps OpenEnv | Profile only | No dataset/evaluators. | Cover via OpenEnv wrapper. |
| **Harvey LAB, TaxCalcBench, Crosby, PostTrainBench** | Research | None | Ontology already names the pressure. | Phase 8 fixtures. |
| **dig.bench** | Native content (hosted) | C8 `digbench_mock` headless (seven kinds, no frames, `/reward` from status). | Live token + agentic MCP = `--paid` / A8 Desktop. `get_session` ≠ checkpoint. | **A8 capstone.** Do not Harbor-wrap. Do not OpenEnv-wrap. |

### 2.4 Sidecars + Workshop visuals

```text
Workshop
  LagunaManager          → inference sidecar (already shipping)
  OptimizerManager       → optimizer sidecar / Synth Cloud  (to build, same shape)
  Container/eval bind    → Environment + Policy + Relay
  Visual registry        → signed templates, persist-before-publish
        │
        ├─ optimizer_event.v1      search / training / checkpoints / campaigns / links
        ├─ trace-stream-event.v1   env steps, policy spans, rewards, frames
        └─ Trace V5                seal / replay / drill-down
```

Stopping a sidecar must not delete mirrored events. Record `sidecar_version`, `algorithm_version`, `recipe_version` independently. Compose `optimizer-gepa` is opt-in packaging of the same contract.

**Visuals:** connect → ready → start. Scrub rewinds curves, checkpoints, rollouts, and decisions together. No invented fields. Broken frames are evidence failures. SFT family: `optimizer.sft.{live,checkpoints,rollouts,examples,dataset,lineage}.v1`. Eval/Craftax: graduate local HTML to versioned templates on the real stream. Private recipes live in `<evals>/.synth/workshop/` as generic extensions.

**SFT (locked):**

```text
SFT optimizer run → training job → checkpoint → evaluation campaign
  → checkpoint rollout → eval stream → Trace V5
```

Three viewer layers: OpenAI-compatible FT baseline; Synth checkpoint-rollout (paired lanes, split roles, promotion ≠ “checkpoint ready”); durable transport/replay after the provider disappears. `optimizer_event.v1` is enough as the envelope; current SFT projections are not. Acceptance is a real hosted multi-checkpoint job opened **before** training.

---

## 3. Code gaps

**Containers.** Accept `poll`; return bound transport; fail if the required transport is impossible. Replace snapshot-diff SSE/WS with an append-only log. Cursor on `GET .../events`. First-class env/policy/harness/task_world. Logical service IDs on `/metadata`. Harbor owned adapter + one public fixture. OpenEnv Echo gateway; stop advertising unproven snapshots. Prime stub + non-upload harness only. Craftax EnvironmentService façade over gold HTTP. Land `docs/specs/trace-streaming-profile-v1.md` + schemas + `tests/conformance/trace_stream/` as specified in `live_evals.md`.

**Optimizers.** Fail missing sequence; never default usage/reward to 0. One spool per run. MAPO/OHCO map into `optimizer_event.v1` or stay non-submittable. Finish standalone SFT campaign/rollout/promotion + aligned metric slices. Keep `goex.sft.v1` namespaced. Child evals are resource refs.

**Workshop.** Persist-before-publish. Bind visuals to declared stream IDs, not constructed URLs. Fix Craftax `stream` slot. OptimizerManager beside LagunaManager. Do not wait on Specta codegen. Do not leak private runner names into templates.

**Evals.** Once §2.1 exists, emit the shared envelope into a Containers relay (keep the gold client if needed). Private extension source stays in Evals.

---

## 4. Implementation cut

Status 2026-08-12. Headless Containers §12 is the ship gate. Desktop / `--paid` / hosted SFT are the then-column.

| Step | Work | Status |
| --- | --- | --- |
| 0 | Freeze wire: telemetry + `policy_ref` `{harness,config,code?}`; stream-event trait; missing≠0 | **Done** (docs + containers floor) |
| 1 | Durable log poll + SSE + optional WS; sequence cursor; `stream.subscribed`; no `auto` | **Done** C1 / A5 headless |
| 2 | Env/policy/task_world; Craftax 10× through Containers HTTP; visual before step 1 | **Headless done** (`examples/craftax_ten_seeds.py` + C3). Eval driver: prepare → `stream.subscribed` → start. Desktop TS-E01 + paid Luna = nightly |
| 3 | Harbor fold v1: public fixture, verifier ≡ `/reward` script node, ATIF projection, `live.harbor_eval.v1` | **Fixture + ATIF projection + C4-06 hillclimb DAG done.** Docker Harbor + in-app register = A2 Desktop |
| 4 | OpenEnv Echo wrap | **Out of this cut.** |
| 5 | Optimizer sidecar + two GEPA (Banking77) persist-before-publish | **Done — A3 receipt.** Candidate + Pareto projections and `cost_usd` fixed by that run |
| 6 | Standalone SFT campaigns / aligned metrics | **A4 done** (two hosted Tinker runs, distinct digests). **A6 partial** — campaigns run through `banking77_classify` but score `null`; the container has no local Tinker sampler |
| 7 | **A8 dig.bench** `live.digbench.v1` first, two harnesses, no frames | **Blocked on `DIGBENCH_API_TOKEN`.** C8 mock headless remains the only path exercised |

Next cut: Prime GSM8K, Chess OpenEnv, private GameBench/CardBench extensions, MAPO/RLVR/OHCO, GEPA on dig.bench.

---

## 5. Open questions

Still from the master plan: (1) OpenEnv example after Echo, (2) in-container gateway vs sibling sidecar, (3) frame/token retention, (4) rescore = child execution vs derived receipt, (5) native/wrapped tolerance, (6) upstream version window, (7) extension powers without a second confirm.

New (closed 2026-08-12 by [`PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md`](./PLAN_OPTIMIZERS_WORKSHOP_VISUALS_2026-08-12.md)):

8. Wrap Craftax whole-log poll as `cursor=nev_cursor`, or always materialize a sequence log in the relay? **Sequence log in the relay.** Workshop never speaks `nev_cursor`.
9. Forbid `telemetry.transport=auto` on authoritative runs? **Yes** when a visual is attached / the run is authoritative.
10. HarnessService as the shared Luna-med-10× recipe (recommended). **Superseded:** recipe is `policy_ref` `{ harness, config, code? }`. No sibling `HarnessService`.
11. Is `synth.stream-event.v1` an outer envelope or a trait flattened into `optimizer_event.v2` / `trace-stream-event.v1`? **Trait.** Two concrete schemas this cut: `optimizer_event.v1` and `synth.trace-stream-event.v1`. Do not require an outer wrapper.
12. A8 dig.bench: which public game; mock vs `--paid`; both harnesses in Desktop? **Freeze one game on the receipt (P-1 or first `list_games`).** PR mock may skip agentic MCP; nightly `digbench_public` must not. A8 Desktop needs **both** harnesses. C8 signed off.

---

## 6. Success bar

Today’s CLI slice: 10 concurrent Luna med seeds, 580 steps, Σ reward +33.10, 1 zombie death, $0.083, 82% cache, sealed V5 at `containers/temp/_runs/luna_med_10x/`.

This update is done when that same job is: launched through Containers with env/policy/task_world and an explicit stream bind; Workshop visual connected before the first paid call; poll/SSE(/WS) equivalent; spans open/data/close; `capture.closed` then a reconciled Trace V5 that still correlates observation, action, reward, and policy call — without inventing fields and without naming private Evals runners in public surfaces.

**Final bar (A8):** the same contracts, in Desktop, on a public [dig.bench](https://digbench.ai) game: visual first, two harnesses, hosted env relay, text evidence only, `/reward` from win/loss, reopen after their session is gone. If A8 needs a second stream, a Harbor wrap, or invented frames, A1–A7 were not actually done.
