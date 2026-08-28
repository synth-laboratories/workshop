# live.craftax.v1

Craftax's canonical immersive gameplay viewer. It is derived from the real-stream
reference dashboard and includes a dominant gameplay surface, video-like playback
over the ordered PNG frames emitted by Containers, an environment and ReAct policy
sidebar, evaluation- and rollout-time replay, cumulative reward and achievement
plots, ordered activity, and a selectable full Trace V5 viewer.

Bind slot **`stream`** to the exact descriptor returned by
`container_prepare_rollout`. Frames render only when a real `frame` event is
present. The image player supports frame scrubbing and 2/4/8/12 fps playback;
symbolic fallback uses the real observation grid only when no frame URL exists.
The trace viewer defaults to Full trace (lifecycle, observations, frames, policy,
actions, rewards, achievements, status, and reconciliation) and can focus on just
policy partials. Missing reward, usage,
cost, and vitals display as —. No invented map, policy reasoning, or terminal state.

Create with `presentation: "canvas"`. Trusted configuration lives in visual
metadata under `visualConfig`:

```json
{
  "theme": "ember",
  "density": "comfortable",
  "showPlots": true,
  "showActivity": true,
  "showTraceInspector": true
}
```

Before starting a paid or policy-backed rollout, record two passing rendered
reviews at different viewport widths and mark the current revision ready.
