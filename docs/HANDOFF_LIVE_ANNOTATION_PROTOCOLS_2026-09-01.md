# Handoff: live incremental annotation protocols (lane C)

**Date:** 2026-09-01
**Status:** implemented and proven on all three repos. Container lane proven on the real Craftax Rust engine in both directions (three parallel rollouts, annotation SSE tailed while running; consumer controls applied mid-rollout: note, hot-swap with state, live threshold change, stop, two refusals). Workshop configure / pin / relay / auto-bound viewer / control IPC / post-seal reconciliation are unit-proven; a Workshop-driven run against a rebuilt image is the one thing not yet executed.
**Run it:** `docs/RUNBOOK_LIVE_ANNOTATION_GLM_CRAFTAX_2026-09-01.md` (GLM 5.3 flash policy + GLM judge, live viewer, validity checklist).
**Builds on:** `docs/HANDOFF_ANNOTATIONS_POSTHOC_ARCHITECTURE_2026-09-01.md` (this is the "observe-only provisional lane" that document recommended).

---

## Product sentence

Twenty Craftax rollouts streaming in parallel are unreadable. A live annotation protocol is caller-supplied, digest-pinned code that runs beside each rollout inside the container, consumes the rollout's event stream as it grows, and emits its own incremental stream of provisional annotations: achievements as they unlock, milestones as they are reached, failure modes as they are detected, and bounded model judgments. Workshop configures the protocol per recipe, pins it like policy code, relays the annotation stream into the run journal, and shows it as a summary layer over the underlying rollout events.

## Causal boundary (unchanged from the post-hoc handoff)

The protocol is observe-only. It cannot see the policy, the policy cannot see it, and it cannot alter reward, achievements, terminal status, or the sealed Trace V5. Every finding is `provisional`; the post-hoc `[annotation]` stage over the sealed trace remains the evidence authority. Retractions and supersessions are shown, never erased.

---

## Branches (all unpushed)

| Repo | Worktree | Branch | Base |
| --- | --- | --- | --- |
| containers | `~/GitHub/containers-live-annotation` | `josh/live-annotation-protocol` | `b8e6490` (`josh/annotations-list-achievements`) |
| evals | `~/GitHub/evals-live-annotation` | `josh/craftax-live-protocol` | `56f4cd038` (`agent/workshop-evals-v04`) |
| workshop | `~/GitHub/workshop-live-annotation` | `josh/live-annotation-workshop` | `99ea8f6a` (`codex/v0.9.0`) |

Two branch-hygiene facts you need before running anything:

- containers `b8e6490` does not import on its own: `platform/state.py` imports `platform/trace_bundle.py`, which is **untracked** in `~/GitHub/containers` (the sibling tree keeps a version that differs from `main`'s). The worktree carries an untracked copy; the branch commits only the `http_requests.py` parser hunk (byte-identical to the sibling tree's uncommitted diff, so it merges clean). Whoever lands `josh/annotations-list-achievements` must commit `trace_bundle.py`.
- The Workshop vendored owner schema (`contracts/optimizer-event-v1/schema.json`) now declares `eval.trial.annotation`; the byte pin in `event_contract.rs` was updated. The Optimizers repo (`contracts/event_vocabulary.json`) has not adopted the type yet; its parity test derives the vocabulary from emitters in that repo, so adopting it is a design decision there (Workshop's relay is the emitter).

---

## Containers: `synth_containers.live_annotation`

Spec: `docs/specs/live-annotation-protocol-v1.md` (contract, kinds, ordering, durability, selectors). OpenAPI: `/annotation-protocol`, `/rollouts/{id}/annotations/events`.

- `PUT /annotation-protocol {code, protocol_id, configuration?, source_revision?}` → `anprev_<sha16>` revision, idempotent, boots the code once in isolation before accepting, refuses credentials.
- `POST /rollouts/prepare|rollouts` take `annotation_protocol_revision_id`; the descriptor then declares `stream.annotation = {events, stream, id, kinds, status: provisional}`.
- Protocol host: `IsolatedProtocolProcess` = `python -I -S`, scrubbed env, JSONL. Stdlib-only, provably (test imports the container package from the child and fails).
- Runner: one thread per rollout tails the same `RolloutEventLog` a remote consumer polls (`wait_for_change` added to the log), feeds the child, validates emissions, brokers model requests with ceilings, seals its own stream (`annotation.closed`, `capture.high_water`, `capture.closed`).
- Attach point: `CompatPlatform._simulate` wraps `runtime_for(spec).simulate` — runtime-family agnostic, no policy or world handle.
- Advertisement: `/info` → `capabilities.operations["annotation.live"|"annotation.protocol.put"]`, `live_annotation` block.

Tests: `tests/test_live_annotation_{process,runner,http}.py` (18 tests). Full suite in the worktree: 460 passed, 4 failed — the four `test_container_compat_conformance` cases fail identically on pristine `b8e6490`.

## Evals: `craftax.live.v1`

`domains/craftax/annotations/live_protocol.py` is the protocol (stdlib-only, self-contained; the milestone table is embedded and a test pins it to `milestones.py`). `domains/craftax/annotations/live.py` gives the install body, an in-process replay, and a seal→events projection.

Emits: engine-verified `achievement` findings (readout list or `achievement_unlocked` events); `milestone` findings for engine-gated and inventory-gated nodes (inventory nodes wait for their measurable prerequisites — the vacuous-gate bug from the post-hoc lane does not recur); provisional `failure_mode` findings for repeated blocked actions (superseded as the streak grows), noop streaks (`ignored_threat` when a hostile is near), oscillation with no progress (retracted when progress follows), low health (retracted on recovery), neglected vitals; metrics (`cumulative_reward`, `health`, `achievements`, `plan_length`, `judge_progress`); and, when `configuration.model` is set, one bounded judge request per `judge_every_calls` plans returning `failure_mode` / `intent` findings with `basis: model`.

Tests: `tests/test_craftax_live_protocol.py` (10 tests, incl. replay of the real seal `roll_ab9de205861d` in-process and inside the isolated host; run with `PYTHONPATH=~/GitHub/containers-live-annotation/src` for the host test).

## Bidirectional streaming

Three parties, two directions each way, one durable authority per stream:

| Direction | Transport | Contract |
| --- | --- | --- |
| rollout → annotator | in-container tail of the rollout's durable log (`wait_for_change`) | same `after(sequence)` cursor a remote consumer uses; observe-only |
| annotator → consumers | poll (authority) · SSE · duplex WebSocket, all declared on `stream.annotation` | `synth.live-annotation-stream.v1`, own sequence space, `stream_id` on every row |
| consumers → annotator | `POST …/annotations/control` or the same JSON on the WebSocket | `synth.live-annotation-control.v1`: `message` (→ `on_message`), `protocol.update` (hot-swap, `snapshot`/`restore` carries state), `stop`; immediate ack to the sender, durable `annotation.control.received/refused` on the stream for everyone |
| annotator → rollout | **none** | findings are advisory evidence; a harness acting on them is a separate, versioned feature (it would put annotation truth upstream of reward) |

Workshop speaks the consumer direction through `annotation_control_send` and `annotation_protocol_update` (install → optional run-pin advance → optional hot-swap of running rollouts); the eval worker re-reads the run's pin per dispatch. The viewer shows control acks, refusals and rebinds as lane history.

## Post-seal reconciliation

`session/live_annotation_projection.rs` folds the relayed annotation rows per rollout (supersede/retract history intact) into `annotation_provisional_findings` and, after lane B, checks every citation against the rollout's relay-verified journal (closed, contiguous, digest-checked; the same evidence the seal was built from) and the sealed post-hoc labels for the same trace: `resolved`, `corroborated`, `unresolved` (a citation the verified journal does not contain), or `unsealed`. Recorded as an `optimizer.evidence.amended` amendment; read back with `annotation_provisional_list`. Provisional rows keep their own table and vocabulary and never become sealed findings.

## Workshop

- Recipe: `[live_annotation] protocol_id, protocol_source, [configuration], [model]` → `optimizers/live_annotation.rs::LiveAnnotationSpec` (closed key sets, credential refusal). Bundled `recipes/annotation_eval/eval.craftax.gold.live_annotated.v1.toml` declares both lanes.
- Pin: `register_protocol_pin` mirrors the NanoHorizon policy pin (GET → PUT on mismatch → GET → refuse without `protocol_revision_id`), persisted as `summary.liveAnnotationPin`; gated on the refreshed `/info` advertisement (`live_annotation_unsupported`, fail-closed).
- Rollouts: `run_one_example` stamps `annotation_protocol_revision_id` on prepare and start, resolves `stream.annotation.events` (never guessed), and refuses a pinned rollout whose descriptor declares no channel.
- Relay: `eval_relay.rs` drains the annotation stream beside the reward lane with its own cursor and de-dup set, emits it on the owner's `eval.trial.event` carrier tagged `delta.stream = "annotation"` (`idempotency eval:annotation:{rollout}:{sequence}`, `container_event.stream_id = stream:<rollout>:annotations`), and gives the stream a 45 s grace to seal after the rollout journal. `RelayOutcome` reports `annotationEventsRelayed`, `annotationFindingsRelayed`, `annotationClosed`.
- Live pane: when a recipe declares `[live_annotation]` the worker binds SSE, mints `live.annotated_rollouts.v1` (role `live_annotation`) with the run, and appends each rollout's declared stream and annotation sibling (`source` + `poll_url`) after prepare under a per-run lock; the pane settles with the run. The MCP prepare route returns `annotation_visual_binding` beside `visual_binding` and passes `annotation_protocol_revision_id` through prepare and start.
- IPC/MCP: `annotation_protocol_get`, `annotation_protocol_update`, `annotation_control_send`, `annotation_provisional_list` (shim catalog and host table kept equal by the contract test).
- Capability: `container_capabilities.rs` records `annotation.live` / `annotation.protocol.put` verbatim from `/info` without affecting `complete`.
- Visual: `visuals/families/first_class_example_containers/live.annotated_rollouts.v1` — one multi-stream `stream` input; bind each rollout's stream **and** its annotation sibling. `project.ts` folds both by `rollout_id` (rows de-dup by `stream_id`), keeps the underlying step/reward/vitals/achievements, and layers findings (chips by kind, confidence, judge basis, superseded/retracted history), per-step markers, judge metrics, and cross-lane tallies; relayed `eval.trial.event` / `eval.trial.annotation` envelopes unwrap to the same reducer. Fixture: `visuals/fixtures/live_annotated_rollouts_craftax.json` (real seal + one synthetic lane, both annotated by the real protocol in the isolated host).

Tests: Rust unit tests in `live_annotation.rs`, recipe seeding/parse, relay/capability/event-contract modules (87 pass); `visuals/tests/live_annotated_rollouts.test.mjs` (5) and the full visuals suite (268 pass). `container_eval::tests::a_stopped_container_does_not_take_the_relayed_replay_with_it` fails on this branch and on pristine `99ea8f6a` alike (mock `/health` 404).

---

## Proof run (2026-09-01)

`containers-live-annotation/scripts/live_annotation_craftax_e2e.py` ran the compat façade in-process on the host (target `craftax_code_policy` from `main`'s image tree, heuristic code policy, no model key) against the live engine on `127.0.0.1:18098`, with `craftax.live.v1` installed (`anprev_e06808dc5480009e`) and three rollouts prepared, subscribed on both declared SSE streams, then started concurrently.

| rollout | steps | reward | rollout events | annotation events | findings | first annotation after start | annotation stream sealed after rollout's last event | poll page == SSE |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| seed 0 | 60 | 1.0 | 765 | 205 | 15 | ~0.1 s | +0.26 s | yes |
| seed 1 | 60 | 2.1 | 579 | 157 | 16 | ~0.1 s | +0.26 s | yes |
| seed 2 | 60 | 0.1 | 415 | 111 | 11 | ~0.1 s | +0.13 s | yes |

Findings were incremental (over 90 % of annotation events arrived before the rollout's last event), ordered by source sequence, superseded at escalation thresholds, with zero protocol errors, `outcome: completed` on every stream, and the poll authority reproducing the SSE digests exactly. The protocol correctly diagnosed the heuristic policy: `collect_sapling` achievements, `feedback_incorporation.repeated_blocked_action` for `do` with engine reason `no_sapling` (57 of 60 steps), and `safety_survival.low_health` on seed 2. The run also caught one contract bug, now fixed and tested: the annotation stream must exist from prepare so a viewer can subscribe before start.

## What is not done

1. **Workshop-driven run against a real container.** The container lane is proven from the host in both directions; the Workshop path (recipe → pin → relay → auto-bound pane → control → reconciliation) is unit-proven with mock containers only. Driving it needs a running desktop (the MCP shims talk to `visuals-ipc.json`; there is no headless eval binary) and a Craftax image rebuilt from a tree that carries `live_annotation` (the image lives on containers `main`, the lane on the annotation branch), with a provider key in the container env for the judge.
2. **Judge on a real run: done.** GLM 5.3 flash as policy and judge, 2026-09-01, judge 5/5 parsed (see the runbook). Remaining: a real judge through the Workshop proxy rather than a host env key.
3. **Reconciliation semantics beyond citations.** Corroboration matches labels between the live taxonomy (`criterion.mode`, milestone ids) and sealed post-hoc labels; a Craftax post-hoc annotator that confirms provisional findings explicitly would make `corroborated` precise.
4. **Vocabulary ownership.** Annotation rows ride `eval.trial.event`; readers of that carrier must check `delta.stream` (the viewer does). If Optimizers ever grows an emitter for a dedicated type, switch the relay.
5. Skills (`trace-v5-annotate`, `run-live-container-evals`) mention the lane; the MCP tool descriptions carry the rest.

## Definition of done for this lane

- A protocol file can be updated in the workspace and the next run installs the new `anprev_` revision without an image rebuild (proven by tests; unproven on a real container).
- Every rollout with a pinned protocol declares an annotation channel; every annotation row is durable before it is visible; the stream seals after the rollout journal.
- Findings are provisional, retractable, and superseded rather than mutated; none reach an evidence head from this lane.
- The live viewer shows the annotation layer and the underlying events for the same rollout, from the same fold.
