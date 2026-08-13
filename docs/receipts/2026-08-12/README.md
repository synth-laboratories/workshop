# Aug 12 acceptance receipts

Machine receipts for [`aug_12_update.md`](../../aug_12_update.md) A1–A8. Every
row here is a run that happened on 2026-08-12, not a fixture. Where something
did not happen, the row says so.

Workshop instance `livecraftax` at `b087418a7c72-dirty` plus the uncommitted
fixes listed below. Containers façades on loopback; optimizers-beta on
`127.0.0.1:8881`; hosted training on Tinker.

This index preserves the original Aug 12 run notes. The later
[`w1-w3-cua.json`](./w1-w3-cua.json) receipt supersedes the original W1–W3
row only: W1 and W2 passed on isolated dirty-tree Desktop instance `w1final`;
W3 has one passed safe-stop drill and three quota-blocked drills. It does not
close the installed-binary Tier 4 gate. The clean-candidate preparation and
auth blocker are recorded separately in
[`../2026-08-13/w1-w3-tier4-preflight.json`](../2026-08-13/w1-w3-tier4-preflight.json).

| Receipt | Test | Result |
| --- | --- | --- |
| [`a1.json`](./a1.json) | A1 Craftax Luna med 10× | **PASS** — 10/10 paid lanes |
| [`a3.json`](./a3.json) | A3 two live Banking77 GEPA | **PASS** — 5/5 assertions |
| [`a4.json`](./a4.json) | A4 two hosted Tinker SFT in parallel | **PASS** — 8/8 assertions |
| [`a5.json`](./a5.json) | A5 durable stream contract | **PASS** — 8/8 checks |
| [`a6.json`](./a6.json) | A6 hosted multi-checkpoint SFT campaign | **PARTIAL** — 7/7 structure checks; campaign rollouts scored `null` |
| — | A2 Harbor Docker GameBench | **NOT DONE** |
| — | A8 dig.bench capstone | **BLOCKED** — no `DIGBENCH_API_TOKEN` on this machine |
| [`w1-w3-cua.json`](./w1-w3-cua.json) | W1–W3 agent golden path with GPT-5.6 Sol | **W1 PASS / W2 PASS / W3 PARTIAL** — 10/10 real Craftax rollouts and a pre-start visual revision passed on dirty-tree CUA; missing-Visuals-MCP stopped safely; poll-503, frame-404, and policy-pin-refusal remain quota-blocked |

A7 (OpenEnv Echo) is out of this cut.

---

## A1 — `CRAFTAX-LUNA-010` / TS-E01

Batch `craftax_luna_010`, seeds 0–9, `openai/gpt-5.6-luna` at medium effort
through the `craftax_react` Containers façade over the rust gold service at
`127.0.0.1:18100`.

- Registered in Workshop as `ctr_09e158f07c2d45249e47b29db6012ed4`; register
  classified `family: craftax` → `live.craftax.v1`, slot `stream`.
- One visual (`vis_fd4915fa93454fadae79addea5c952be`) bound to the **declared**
  SSE URL before every lane's first paid call; `stream.subscribed` observed
  before each `POST /rollouts`.
- 792 environment actions, 104 paid Luna calls, 358,632 tokens, **$0.0311**.
- Rewards `[4.0, 3.0, 2.8, 1.1, -1.0, 1.6, -0.9, -1.0, 0.1, 0.1]`, Σ +9.80.
- `policyAuthority: "container"` on all ten — Workshop never called the model
  or `/step`. `recovered: false` on all ten, so none is a replayed lane.
- Every lane persisted before publish; `spoolDigest` recorded per lane.

The Σ reward is well below the 2026-08-12 CLI slice's +33.10. Same policy pin,
different episode conditions — this receipt is a contract receipt, not a
score comparison, and no claim is made that the two are equivalent runs.

## A3 — two live Banking77 GEPA, Luna vs Sol

`gepa.banking77.luna.v1` and `gepa.banking77.sol.v1` started back to back
through Workshop's optimizer surface, both `algorithm_id: "gepa"`.

- Distinct `optimizer_run_id`s, disjoint event logs (13,270 and 11,469 events),
  distinct visuals, both on `optimizer.gepa.live.v1`, 240 rollouts each.
- Four visual flips during the run; the unfocused lane advanced every time
  (e.g. Sol 536 → 757 while Luna was focused; Luna 8,270 → 8,770 while Sol was
  focused). No flip stalled the other lane.

Three defects the run surfaced, all fixed and covered by tests:

- `gepa.candidates` projected empty because it keyed off `event.item`, which
  registration events do not carry.
- `gepa.frontier` projected empty because it only read a `cells` array; the
  sidecar reports the frontier as `best_candidate_id` + coverage.
- `OptimizerUsageSummary.cost_usd` was a non-optional `f64` that read `0.0`
  for a run nobody reported cost for.

Both slices now populate on the completed runs (best candidate
`gepa_a0934d01bd05`, 80% train coverage, up from the seed's 70%).

## A4 — two hosted Tinker SFT runs in parallel

Two `sft.banking77.nemotron-lightning.tinker.v1` runs, shards `train_a` and
`train_b` — disjoint halves of the pinned Banking77 corpus.

- Both `algorithm_id: "sft"`, `banking77-nemotron-lightning-tinker-v1`, never
  `goex.sft.v1`.
- Both **completed**, live at the same time.
- Distinct dataset digests (`sha256:33d7f956…` vs `sha256:1fec1173…`), distinct
  event logs, distinct visuals, three checkpoints each.
- `costUsd: null` on both — nothing reported cost, so nothing invented one.

Three defects fixed to get here:

- optimizers-beta read `stream.poll_url` while the frozen Containers descriptor
  declares `stream.transports.poll.url`, so every campaign failed with
  `containers prepare omitted poll_url`.
- Workshop's hosted worker ended the mirror on a single transient upstream read
  and reported a producer-**succeeded** run as failed.
- That terminal event carried no reason at all; the cause went to stderr.

## A5 — durable stream contract, on the A1 paid stream

All eight checks on real paid rollouts, not a fixture server.

| Check | Evidence |
| --- | --- |
| poll (7-row pages) ≡ SSE | 2,856 events, identical ordered `(sequence, kind, digest)` |
| poll ≡ SSE ≡ WebSocket | 2,753 events on the WS-bound paid lane, all three identical |
| control does not advance evidence | `stream.subscribed` carries no sequence; heartbeats are not events |
| reconnect by `Last-Event-ID` | resumed tail matches poll tail exactly, no gap, no duplicate |
| `transport=auto` refused | prepare rejects it |
| no silent degrade | a rollout prepared as `sse` advertises `websocket: null` and refuses a WS subscribe |
| missing ≠ 0 | terminal `/reward` on an unstarted rollout refuses; reward stays `null` |
| reopen after the container is gone | façade killed; declared poll URL unreachable; the run still reopens from the Workshop spool blob on disk |

## A6 — hosted multi-checkpoint SFT campaign (partial)

Run `sft_banking77_train_a_407b5131`. All seven structural checks pass:
visual open before `sft.training.started`; three checkpoints each with an
allocated and completed campaign; campaigns run through `banking77_eval.v1`
against the `banking77_classify` container; six unique child rollout ids
carried as `synth.resource-ref.v1` `container_rollout` refs with role
`candidate_evaluation`; aligned metric points rather than parallel arrays;
promotion distinct from `checkpoint.ready`; and the whole run still reopens in
Workshop after the optimizers-beta producer is stopped.

**What does not pass:** every campaign rollout scored `0 / 2` with
`reward: null`. The child rollouts are real — real Banking77 heldout rows, real
`classify` harness, subscribe-first, sealed capture — but the checkpoint policy
cannot be sampled in this environment. The container's Tinker sampler needs a
local torch/transformers stack it does not have, so the policy span closes
`failed` with `ImportError`. Null is the honest answer here, not a fabricated
0, but A6 is not a pass until a checkpoint actually classifies.

While chasing this, the Banking77 runtime was changed to carry a secret-free
`error_code` on `span.policy.closed` — previously the stream said only
`RuntimeError`, which a reader cannot act on.

## A2 — not done

`gamebench-harbor-code_policy_deo_hillclimb-craftax-singleplayer:latest` is a
real Harbor-packaged GameBench task and its agent and verifier roles run as
distinct `docker run --rm` executions. But `harbor_docker.py` still only knows
the alpine public fixture; it has no path for a pinned Harbor bundle, so
nothing about this reached Workshop. Two findings from the direct trial:

- the verifier needs the GameBench task tree at
  `/workspace/gamebench/tasks/<task>`; the bundle does not carry it
- on a missing hillclimb runner the bundle's own `test.sh` writes
  `reward 0.0`, which is a missing-≠-0 violation inside the Harbor package

## A8 — blocked

`api.digbench.ai` answers `401 {"detail":"authentication required"}` and no
`DIGBENCH_API_TOKEN` exists anywhere on this machine. The mock path (C8) stays
headless-only. Nothing was faked in its place.
