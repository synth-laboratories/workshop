# Optimizers-owned event contract (vendored consumer copy)

This directory is a byte-for-byte consumer copy of the Optimizers-owned
`optimizer_event.v1` schema and generated eval-worker corpus. Workshop does
not own or extend this vocabulary.

- Upstream repo: `synth-optimizers`
- Upstream commit: `6482d1b6e07784f1c526e56a68c534c7663da316`
- `schema.json` SHA-256: `52d37ceda9a1c37045af3417314bc2be3709c8588a007d511ed85b53a2d5430e`
- `fixtures/eval_worker_events.jsonl` SHA-256: `0f65ef3d236c57bb69debf9373117519a1190de2ac80d0ee731777bb74d3c95b`

Workshop normalizes imported `eval.worker-event.v1` events and relayed
`eval.trial.event` carriers into the owner's snake_case wire projection and
validates them immediately before the durable append. Workshop-local
lifecycle, settlement, evidence, SFT, and training events stay on Workshop's
local contract path.

`delta.container_event` is the only producer spelling. Reads remain tolerant
of legacy `delta.containerEvent` rows during the compatibility window.

**COMPAT removal flag:** remove the `containerEvent` read alias after the
release following 2026-08, at the same time Optimizers removes
`$defs.container_event_carrier.properties.containerEvent`. Search for
`COMPAT_CONTAINER_EVENT_CAMEL_CASE_THROUGH` to find the guarded read paths and
regression tests.
