# v0.4 installed acceptance

## Exact app

- Workshop source: `9fffe8c8b5ede969b734118c04935fe42cc6baf1`
- Product/version: Synth Desktop 0.4.0
- Optimizer runtime: synth-optimizers 0.2.14, ready
- Artifact SHA-256: `a1f2e882ccc7ac4eeab31ce55b1548a11114cd6b3c10f5290a4e94cecaa114ec`

## Banking77 GEPA v2

- Run: `banking77_gepa_luna_med_8c1278ef`
- Preparation digest: `sha256:9af1387f7f6e70da49bfe823457478b2ef0d9e1325f6d9c33d4c1db33fc1d1b8`
- Visual: `vis_c519178fd8d3455caa4942ad0bb55da8`
- Status: completed
- Cost: $0.0101283 against the $2.45 recipe ceiling and $20 operator authorization
- Rollouts: 140 scored, 0 failed evaluations
- Candidates: 2 registered; 1 retained frontier member
- Best/seed: `gepa_1c284a9e221e`, train 0.76, heldout 0.60
- Proposal: `gepa_bafc1d715ea0`, rejected at the minibatch gate (0.85 versus parent 0.90)
- Usage: 150,006 tokens, 140 policy calls, 1 proposer call
- Recorded visual elapsed: 5m 24s; proposer wall time: 130.2s

## UI review

- The terminal optimizer visual rendered cost, elapsed time, candidate list, one-member Pareto frontier, all 140 evaluations, and the complete Generation 0 Trace V5 with 11 items and 5 tool calls.
- The chat advanced-trace panel rendered the final Responses API v5 receipt with 250 provider events and displayed a selected `turn/completed` payload.

The installed-artifact, paid-run, TPS/elapsed, candidate/frontier, proposer-trace, and full advanced-trace acceptance gates are satisfied.
