# craftax.rollout_scrub.v1

Frame scrubber + HUD + **accessible text projection** (PostTrainBench spirit, Craftax frame chrome).

Never relies on pixels alone — `observation_text` is always shown beside / under the canvas.

## Slots

| Slot | Accepts |
| --- | --- |
| `rollout` | fixture, local_cas, trace_v5 |

## Accessibility

- Scrubber: `role="group"`, `aria-valuetext`
- Frame: `role="img"` with projected caption
- HUD vitals: labeled progressbars
