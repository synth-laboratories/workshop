---
name: run-live-container-evals
description: Run and verify live Synth container evaluations with opt-in SSE telemetry, real rollout evidence, and the live.container_rollouts.v1 Workshop visual.
---

# Run live container evals

Use `synth_containers` for container discovery and rollout requests and
`synth_visuals` for the live view. Do not scan ports or substitute fixtures.

## Workflow

1. Call `container_list`, select a registered container, then `container_probe`.
2. Confirm `/info` advertises `rollout_stream_sse` or the rollout response can
   return `synth.rollout.stream.v1`. Prefer SSE; use WebSocket for interactive
   control or binary delivery only.
3. List task instances and choose explicit stable IDs/seeds. State rollout count,
   model/policy, limits, and spend before a paid run.
4. Create `live.container_rollouts.v1` through `synth_visuals.visual_manage`.
   Bind slot `stream` as `live_sse` to an absolute SSE URL with schema
   `synth.rollout.event.v1`, then show the visual.
5. Start each rollout with:

```json
{
  "task_instance_id": "craftax:test:2001",
  "telemetry": {
    "enabled": true,
    "transport": "sse",
    "detail": "standard",
    "poll_interval_ms": 500,
    "frame": {"enabled": true, "format": "png", "every_n_steps": 1}
  }
}
```

6. Resolve the returned relative `stream.sse_url` against the registered
   container base URL. Never guess a stream route or rollout ID.
7. Confirm the visible pane receives at least one `snapshot`, advances real
   `progress.env_steps`, and displays the container-provided frame when present.
8. Keep the stream open through `eval.run.terminal`. Report failures by lane.
   Preserve the final sealed Trace V5 identity; a live stream is operational
   evidence, not the durable evaluation record.

## Replay checks

- The evaluation-time slider replays the complete multi-rollout view up to the
  selected wall-clock event.
- Each rollout-time slider independently replays that lane within the selected
  evaluation window.
- The time at the right of each slider must match the selected event.
- Historical frames must use immutable step URLs such as
  `/rollouts/{id}/frames/{step}.png`; never replay a mutable latest-frame URL.

## Acceptance checks

- The event schema is `synth.rollout.event.v1`.
- Use `Last-Event-ID` only when `supports_resume` is explicitly advertised;
  otherwise reconnect as a live-only stream. Terminal events are never inferred.
- Craftax frames come from the returned `frame_url`, not screenshots or fixtures.
- Reward, achievements, vitals, usage, and ETA show missing when unreported,
  never fabricated zeroes.
- For an eval ETA, use the orchestrator's aggregate progress. A single container
  rollout cannot authoritatively estimate the remaining eval queue.

Fall back to bounded polling only when streaming is not advertised, and label
that mode visibly.
