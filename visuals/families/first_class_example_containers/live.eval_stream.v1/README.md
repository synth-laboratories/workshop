# live.eval_stream.v1

Shortcut whole pane for live eval / acceptance SSE (or fixture replay). Host owns ingest via `useLiveEvalStream` + `ReplayClient`. The shell lays out advertised compose parts — `metrics.v1`, `scrubber.v1`, `event_stream.v1`, `detail_modal.v1` — over bind-point `stream`. No compose spec. Do not bind `optimizer_run`.
