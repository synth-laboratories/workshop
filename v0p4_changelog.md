# Workshop v0.4

## New

- Added product-owned GEPA workflows for bounded Banking77 and Craftax optimization, including prepare, digest-bound start, live watch, candidate lineage, frontier, and materialized-result views.
- Added transcript-first Craftax trace inspection with model-call input, reasoning, output, tool evidence, raw envelopes, and honest missing-evidence states.
- Added first-class programmatic eval lanes and container capability checks so paid work fails closed when the selected producer or sidecar cannot satisfy the recipe.
- Added local VictoriaLogs-backed diagnostics correlated across optimizer runs, containers, streams, visuals, and terminal outcomes.

## Improved

- Live visuals now resolve one canonical binding envelope and replay through declared poll transports with durable, lane-local cursors.
- Generation-speed labels are immutable historical snapshots instead of periodically recomputed aggregates.
- Completed turns show elapsed work from the latest accepted turn boundary to the durable terminal event; tool and orchestration gaps do not inflate generation time.
- Review captures use a dedicated window identity, keeping capture sizing policy separate from the product window.

## Fixed

- Reopening or reconnecting a live visual no longer invents stream URLs, collapses multiplexed rollout identities, or rewrites earlier evidence.
- Already-open visual panes reconcile committed revisions without requiring a close/reopen cycle.
- Missing token usage and timing remain unavailable instead of being presented as zero.
- The hosted SFT invariant now checks the current public control-plane and host-owned visual-template contracts instead of a removed implementation detail.

## Known limitations

- This v0.4 friends release is ad-hoc signed and not Apple-notarized.
- Updates remain manual and open the official Synth Desktop download page.
- Paid Banking77 GEPA acceptance and installed-artifact CUA require explicit operator approval and are tracked in the release test report.
