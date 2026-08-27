# Handoff: slime RL → v5 traces → Workshop live view

**Date:** 2026-08-27
**Repos:** `nanohorizon/`, `containers/`, `workshop/`, (later) `optimizers-beta/`
**Audience:** engineers on the trace plane and the eval-live viewer
**Status:** exploratory — the RL side works and is staying in `nanohorizon` for now
**Primary finding:** the facade already emits v5 events over SSE and nothing subscribes

Related:

- `nanohorizon/submissions/fbc-cispo/CISPO_RESULT.md` — the RL result and post-mortem
- `nanohorizon/src/modal_tooling/slime_lane/README.md` — the RL lane, module by module
- `containers/src/synth_containers/event_log.py` — v5 event log, `stream.id`
- `containers/src/synth_containers/platform/app.py` — SSE transport binding
- `workshop/visuals/src/eval-live/model/adapter.ts` — the natural consumer
- `optimizers/temp/gepa_proposer/generated/**/seals/*.trace-v5.json` — reference traces

---

## 0. Why this is worth reading

CISPO RL on Craftax now works: on 40 held-out seeds it took the policy's
invalid tool-call rate from **66.2% to 0.0%** and env reward from **1.320 to
1.873** over 50 updates. The training loop produces ~2,300 episodes per run and
persists **none of them as traces** — only slime's `.pt` training tensors, which
cannot be converted back into traces because they carry tokenised spans, not
request/response content.

That looked like a large piece of missing work. It is not. **The environment
already emits v5 events, and the RL run already asks for them over SSE.** We
just never subscribe.

---

## 1. What already exists

**The facade emits v5 events.** `synth_containers/event_log.py` carries
`stream_id` and writes `"stream.id"`; `nested_runtime.py` and `gold_runtime.py`
already `log.append(...)` for `env.episode.opened`, `span.policy.opened`,
`capture.high_water`. These are the same event types GEPA's sealed traces
contain.

**Every RL episode already requests telemetry.** `nanohorizon/src/core/client.py`
sends this on every `POST /rollouts`:

```python
"telemetry": {"enabled": True, "transport": "sse"}
```

The platform supports `poll | sse | websocket | auto` and gates delivery on
`transport_is_bound(rollout_id, "sse")` (`platform/app.py:241`). So at 128
episodes per training step, there are already 128 live SSE streams of v5 events
that no client has ever opened.

**v5 is a stream format, not a document format.** A GEPA seal looks like this:

```json
{
  "schema_version": "synth.trace.v5",
  "trace_id": "roll_02a2de916314",
  "stream.id": "stream:roll_02a2de916314",
  "rollout_id": "roll_02a2de916314",
  "high_water": 9,
  "capture.closed": true,
  "closed": true,
  "content_digest": "sha256:54e3...",
  "pin": { "environment_ref": "...", "evaluation_plan_ref": "...", "policy_ref": {...} },
  "events": [ /* trace.opened, env.episode.opened, observation,
                 span.policy.opened, span.evaluator.closed,
                 env.episode.closed, status,
                 capture.high_water, capture.closed */ ]
}
```

`high_water` is a cursor. `capture.closed` is a terminal marker.
`content_digest` seals it. **A sealed trace is just a live stream that stopped**,
which is the single most useful fact in this document.

---

## 2. The consequence for Workshop

**Live and replay are the same code path.** Because `high_water` is a cursor,
"watch a run in progress" is "replay a trace that has not closed yet." The
eval-live adapter (`workshop/visuals/src/eval-live/model/adapter.ts`) should not
need a separate live protocol — it needs a cursor it can resume from and a
`closed` flag to stop polling on.

That is the recommendation: **do not build a live path.** Extend the replay path
with a resume cursor and let an unsealed trace be the live case.

---

## 3. Design notes for whoever picks this up

**Tap the facade, not the trainer.** Three candidate capture points, only one
works:

| point | verdict |
| --- | --- |
| slime `.pt` dumps | ✗ tensors only; request/response content is not recoverable |
| `sample_adapter.py` | partial — sees LLM calls, not env events |
| **facade SSE** | ✓ complete, and needs no new emission code |

**Telemetry must never apply backpressure.** The entire RL debugging effort
behind this handoff was about the trainer waiting on generation — at one point
the trainer sat idle 82% of every step. A blocking SSE consumer would put a
Workshop viewer on the critical path of a GPU training run. Bounded queue,
drop-oldest, and record the drop count so a gap in the trace is visible rather
than silent.

**Sample deliberately, and put the policy in the manifest.** ~2,300 episodes per
run is too many to seal. A reasonable cut is: all episodes from the first,
middle, and last step; the reward tails; plus a fixed random sample. Whatever is
chosen, **the manifest must state it** — otherwise a later reader treats a
filtered sample as the full population. That is not hypothetical: in this same
investigation a baseline eval read `env=0.000` because 21 of 40 episodes had
silently hit a timeout and the survivors were exactly the ones that quit early.
It nearly became a published number.

**Order and scale, for sizing:** ~0.62 training steps/min, 128 episodes/step,
~80 episodes/min, a few thousand events/min. Small for a sink; the constraint is
latency isolation, not volume.

---

## 4. Open question — verify before building

GEPA's reference traces come from **`nested_runtime`**. The Craftax path used by
this RL work runs **`gold_runtime`**. `gold_runtime` is confirmed to emit
`capture.high_water` and to reference `span.policy.opened`, but **event-set
parity between the two runtimes has not been checked.**

The whole design above rests on that parity. If the gold path is thinner the gap
is additive and small, but it should be measured before anyone commits to a
schedule. This is the first task.

---

## 5. Scope note

The RL code is **staying in `nanohorizon`** for now
(`src/modal_tooling/slime_lane/`, ~4.1k lines, README in the folder). If slime
RL later becomes a hosted optimizer in `optimizers-beta` alongside GEPA, the
integration surface is the trace plane, not the trainer: GEPA writes
`runs/gepa_<id>/seals/roll_<id>.trace-v5.json`, and a slime backend writing the
same artifact shape under `runs/slime_<id>/` would make every existing consumer
— catalogs, proposers, `pack_v5_hub.py`, Workshop viewers — work unchanged.

That is an argument for keeping the trace contract as the seam, and for not
teaching Workshop anything about slime.
