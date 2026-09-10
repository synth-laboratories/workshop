# optimizer.run.v1

Shared optimizer visual shell with algorithm overlays.

## Slots

| Slot | Accepts | Purpose |
| --- | --- | --- |
| `optimizer_run` | `optimizer_run`, `fixture`, `inline` | Run identity + optional event fixture |

## Overlays

- `gepa` — candidate rail, Pareto frontier, reflections
- `go-ex` — phase board, themes, slot execution binding
- `sft` — fixture-only curves/checkpoints until hosted SFT is available
- `dag` — local experiment DAG node table and honest known spend

## Fixture

`examples/gepa_events.json` exercises live-follow, historical scrub, and candidate comparison without cloud calls.
