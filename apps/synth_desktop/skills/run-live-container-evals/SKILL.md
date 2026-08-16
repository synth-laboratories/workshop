---
name: run-live-container-evals
description: Run and verify live Synth container evaluations with declared streaming telemetry, real rollout evidence, and a task-family Workshop visual.
---

# Run live container evals

Use `synth_containers` for container discovery and rollout requests and
`synth_visuals` for the live view. Do not scan ports or substitute fixtures.

## Operator clock (W1–W3)

This is one bind for Craftax, Harbor, and dig.bench. Skipping a step is a
closed failure, not a retry.

1. **Discover the provider.** Call `container_list`, then `container_probe`.
   Read advertised `runtime_family`, transports, `metadata.capabilities`, and
   `metadata.liveEval`. Never guess `http://127.0.0.1:…/events` or
   `/rollouts/{id}/stream`.
2. **Inspect capabilities.** Select only a currently healthy container
   advertising the normalized prepared-rollout protocol and the exact requested
   policy ref. If none exists, stop and report `compatible_runtime_unavailable`.
   Do not try raw engines, alternate ports, archived rollouts, or prior traces.
   Evidence must match the current invocation's rollout IDs and requested seeds.
   Missing sealed Trace V5 means the requested task is incomplete.

   `metadata.capabilities.operations` is tri-state; `unknown` is not
   `supported`. `health` proves liveness, not workflow compatibility, and SSE
   support does not imply prepared-rollout support. Never fall back from a
   selected policy pool to raw Gold. After a preflight failure, do not perform
   shell or repository archaeology as a substitute for execution; prior
   evidence may be reported as prior evidence only and cannot satisfy a new
   live request. Then confirm the declared poll/SSE/WS URLs, slot `stream`
   (never `live` or `jobs`), and `live_frames`. Harbor and dig.bench are
   `live_frames=unsupported`. `live_frames=native` on those families is a
   stop. dig.bench has no frames.
3. **Create and subscribe the visual first.** `live.craftax.v1`,
   `live.harbor_eval.v1`, or `live.digbench.v1` from the classified family.
   Bind slot `stream` only after prepare returns a descriptor. Pre-start
   readiness means the visual exists, the exact prepared descriptor is bound,
   and the stream has acknowledged subscription. Do not require screenshot,
   frame-replay, or post-data quality review before start: those artifacts
   cannot exist until a rollout emits data.
4. **Wait for `stream.subscribed`.** HTTP 200 on GET is not ready. Heartbeats
   do not count. `ready: true` on the control envelope is required.
5. **Refuse start** if the visuals MCP is down, declared poll returns 503, the
   stream URL was guessed, the visual is absent or incorrectly bound, or `stream.subscribed` is
   missing. Do not invent a replacement URL.
6. **Never fabricate evidence.** Missing reward/usage/frames stay missing.
   Incomplete dig.bench `/reward` is null, not 0. Do not draw dungeon frames.
   Do not put `DIGBENCH_API_TOKEN` in logs, bindings, or screenshots.

Harbor register writes `metadata.liveEval` with template `live.harbor_eval.v1`,
slot `stream`, `liveFrames: unsupported`, and two `policy_ref`s (`harbor_fused`
+ `luna_med`, `harbor_fused` + `sol_med`). Open that visual before trial start.

VisualsBench is a Harbor specialization: registration advertises
`benchmarkFamily: visualsbench`, one explicitly pinned `harbor_fused` Codex
policy, and `mcp_bind: synth_visuals`. The outer `live.harbor_eval.v1` still
connects before start; Codex then authors a separate product visual that the
post-exit script node grades. Refuse start if the Codex policy does not carry
the visuals bind. Missing or ambiguous product identity and incomplete export
are rig failure with null reward, never task score `0`.

dig.bench opens `live.digbench.v1` before `start_session`. Two `policy_ref`s on
the same game: basic (`react_legal_actions`) and agentic (`codex` +
`mcp_bind: digbench-mcp`). Token starts at `start_session`.

Craftax's default 10-lane benchmark pins are seeds 0–9 with `environment_ref` /
`policy_ref` / `task_world`, but an explicit seed set in the user's request
always overrides this default. Never silently replace requested seeds with
0–9. The headless twin is containers `examples/craftax_ten_seeds.py`.
Do not claim a paid Luna 10× run from this skill.

## Workflow

1. Call `container_list`, select a registered container, then `container_probe`.
2. Confirm discovery advertises a supported stream transport. Prefer SSE for
   visuals, use WebSocket for interactive control or binary delivery, and use
   bounded polling only when it is explicitly declared.
3. List task instances and choose explicit stable IDs/seeds. State rollout count,
   model/policy, limits, and spend before a paid run.
4. Allocate or preserve one stable `rollout_id`, then call
   `container_prepare_rollout` for every rollout without starting it. Preserve the returned rollout ID,
   stream ID, transport URL, retention, and policy/environment/task pins.
   Prepare fails locally, before any request reaches the container, with
   `container_unhealthy` (repair the pool, then probe), `container_capabilities_stale`
   (probe first), or `container_capability_mismatch` (read `missing` and
   `available_policy_refs`, then select a compatible registered target or stop
   and report `compatible_runtime_unavailable`). None of these is a reason to
   probe another port, register a new record, or switch to a raw engine.
5. Create the task-family visual through `synth_visuals.visual_manage`:
   `live.craftax.v1` for native Craftax, `live.harbor_eval.v1` for a Harbor
   attempt, or `live.digbench.v1` for dig.bench. Bind slot `stream` (never
   `live` or `jobs`) as `live_sse` to the **declared** SSE URL, with the
   declared `poll_url` beside it. Do not construct `/events` or
   `/rollouts/{id}/stream`.

   Use the `bind` operation — it writes the canonical
   `synth.visual-bindings.v1` envelope. For several rollouts on one `stream`
   slot, pass `mode: "append"` with a `bindings` array in a single call. Do not
   hand-build a `{"stream": [...]}` object through `update`: that shape is
   legacy, is upgraded with a warning, and will be refused.
6. Open the visual in canvas mode and consume the stream until its control envelope reports
   `stream.subscribed`. This acknowledgement is non-evidence and does not
   advance the evidence sequence. Refuse the paid/mutating start if it is
   missing. The first Luna call must not happen while the pane is still on
   empty inline bindings.
7. Once subscription is acknowledged, start each rollout with
   `container_start_prepared_rollout`. Pass the prepared identity, exact stream
   descriptor, visual id, `task_instance_id` or `seed`, and an explicit
   `policy_ref`. The host does not pick `luna_med` or a default harness.
   `container_run_rollouts` is scripted engine acceptance only — never use it
   as a ReAct or model eval.

   Registration records identity and location; it does not add endpoints or
   upgrade an incompatible runtime. If capability preflight rejects a record,
   re-registering the same URL cannot repair it. Select a healthy record that
   already advertises the exact contract, or report the structured blocker.

The agent names the pin. Example (not a host default):

```json
{
  "task_instance_id": "craftax:test:2001",
  "policy_ref": { "harness": "react", "config": "luna_med" },
  "telemetry": {
    "enabled": true,
    "transport": "sse",
    "detail": "standard",
    "poll_interval_ms": 500,
    "frame": {"enabled": true, "format": "png", "every_n_steps": 1}
  }
}
```

8. Confirm the visible canvas receives real `observation`, `action`, `frame`, and
   `reward_signal` events as applicable. For a ReAct policy, require
   `span.policy.opened`, `span.policy.plan`, `span.policy.data`, and
   `span.policy.closed`; the `data` partial carries the provider/model, selected
   actions, nullable usage/cost, and bounded retry evidence. Token deltas are
   additional `span.policy.data` records with `delta: true` only when the
   provider streamed non-empty chunks — empty Luna reasoning stays blank.
   ReAct history uses `compact_every=16`; a compact is a mechanical summary,
   not a model-authored rewrite.
9. After current-run data exists, iterate on the viewer, record wide and compact
   screenshot-backed quality reviews, and obtain a current `visual.ready`
   receipt. A template that requires `imageReplay` must be reviewed here, never
   used as a pre-start gate.
10. Keep the stream open through authoritative terminal `status`. Report failures
   by lane. Preserve the final sealed/reconciled Trace V5 identity and verify the
   persisted journal can reopen with the container gone.

If any request or live transport times out, do not allocate a new rollout.
Call `container_get_rollout` with the stable identity. If it is still prepared,
the exact same start may be replayed; if it is running or terminal, do not start
again. Resume evidence with `container_poll_rollout(after=<last sequence>)`,
advance to `next_cursor`, de-duplicate `(stream_id, sequence)`, and stop retrying
when `cursor.closed` is true. SSE may then reattach with `Last-Event-ID`.

## Replay checks

- The evaluation-time slider replays the complete multi-rollout view up to the
  selected wall-clock event.
- Each rollout-time slider independently replays that lane within the selected
  evaluation window.
- The time at the right of each slider must match the selected event.
- Historical frames must use immutable step URLs such as
  `/rollouts/{id}/frames/{step}.png`; never replay a mutable latest-frame URL.

## Acceptance checks

- The stream envelope schema is `synth.trace-stream-event.v1`; task-specific
  facts live in its typed event kinds and payloads.
- A descriptor with `cursor.kind = sequence` supports poll recovery. SSE resumes
  with `Last-Event-ID`; WebSocket backfills through the declared poll URL before
  reattaching. Terminal events are never inferred.
- Craftax frames come from the emitted immutable frame URL, not screenshots or fixtures.
- Reward, achievements, vitals, usage, and ETA show missing when unreported,
  never fabricated zeroes.
- If `synth_visuals` is unreachable, poll returns 503, or a stream URL was not
  declared by prepare, stop. Do not start. Do not guess `/events`.
- GameBench is task/dataset identity, not a source format. Native Craftax events
  use their declared Craftax source format; only Harbor is a first-class fold.
- For an eval ETA, use the orchestrator's aggregate progress. A single container
  rollout cannot authoritatively estimate the remaining eval queue.

Use bounded polling as the recovery authority even when SSE or WebSocket is the
live delivery path, and label a sustained polling-only mode visibly.
