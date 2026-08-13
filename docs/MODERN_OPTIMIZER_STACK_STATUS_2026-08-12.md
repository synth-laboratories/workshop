# Modern optimizer / Workshop stack — reconciled status

**Date:** 2026-08-12  
**Authority:** [`v0p2_systems.md`](./v0p2_systems.md), then this status, then family handoffs.  
**Scope:** GEPA, GELO, standalone SFT, Containers content/folds, Workshop MCP/visuals, and acceptance receipts.

## Outcome

The remaining product code is implemented on four isolated local branches. It is not
published. The implementation shares the v0.2 structure instead of adding compatibility
paths:

```text
optimizer_event.v1 campaign                    trace-stream-event.v1 children
  GEPA proposer/candidates/frontier ──refs──► Banking77 rollouts
  GELO proposer/candidates/frontier ──refs──► Craftax rollouts
  SFT train/checkpoints/campaigns    ──refs──► Banking77 checkpoint evals
              │                                      │
              └──────── Workshop mirror/visuals ─────┘
```

All families retain the six invariants:

1. missing is not zero;
2. visual and declared stream connect before mutation;
3. promotion is not readiness;
4. child evals are resource refs, never flattened into optimizer events;
5. cursors are monotonic and replay does not execute work;
6. credentials never enter events, state, screenshots, or receipts.

## Local branches

| Repository | Branch | Worktree | Implemented |
| --- | --- | --- | --- |
| Workshop | `agent/aug12-modern-stack-completion` | `worktrees/workshop-modern` | Banking77 recipe/shard MCP parity; fail-closed visual-first dogfood/receipt runner; reconciled docs |
| Containers | `agent/aug12-harbor-dock-modern` | `worktrees/containers-harbor-dock` | pinned Harbor bundle execution; private Dock content adapter over the public Harbor lifecycle; remote checkpoint inference consumer |
| optimizers-beta SFT | `agent/aug12-sft-runtime-completion` | `worktrees/optimizers-sft-runtime` | real accelerator occupancy; isolated jobs/spools; hosted Tinker checkpoint sampling endpoint and opaque bearer registration |
| optimizers-beta GELO | `agent/aug12-gelo-native-containers` | `worktrees/optimizers-gelo-native` | native Containers prepare/subscribe/start/poll/reward; child refs; reconnect/backfill; terminal validation; event canonicalization |

The shared GitHub checkouts were not edited by this completion pass.

## Acceptance truth

| ID | Current truth | What closes it |
| --- | --- | --- |
| A1/A5 Craftax | Existing paid receipt passed on the earlier dirty live lane; visual/reopen product floor exists | fresh clean paid receipt if a clean-tip proof is required |
| A2 Harbor | Product path and pinned-bundle fixture implemented | working Docker daemon plus packaged GameBench run in Workshop |
| A3 dual GEPA | Passed in-app on the live lane; `proposer.delta` exists upstream | fresh clean paid run only if re-certifying the branch |
| A4 SFT multiplex | Prior loose receipt passed; strict rerun found false occupancy and shared-façade collision; both are addressed in the local branches | hosted two-job rerun with one genuinely queued as `accelerator_busy` |
| A6 SFT scoring | Visual and child refs existed; scores were null. Hosted checkpoint sampler and Containers consumer are now implemented | Tinker credential + live checkpoint campaign; null stays null on sampler failure |
| A8 dig.bench | mock protocol/conformance and Workshop visual exist | `DIGBENCH_API_TOKEN`, public game, both basic and agentic harnesses in Workshop |
| A11/A12 | reconnect/idempotency product code plus repeatable driver exists | destructive socket/container kill and paid exactly-once receipts |
| V5/V6 | deterministic 100k projection and semantic/a11y contracts exist | browser heap/long-task and formal axe/screen-reader receipts |
| O1–O5/W1–W3 | floor and driver exist | external destructive/fresh-workspace drills listed in `aug_12_remaining.md` |

No unavailable credential, Docker failure, or skipped external drill is reported as a
pass.

## Repeatable dogfood entrypoint

`scripts/modern_stack_dogfood.py` writes the common receipt bundle:

- `receipt.json`
- `requested-stream.json`
- `bound-stream.json`
- `cursor-transcript.jsonl`
- `event-kind-counts.json`
- `run-manifest.json`
- `cost-reconciliation.json`
- `trace-v5.json`
- `screenshots/`
- `cua-findings.json`

Optimizer example:

```bash
python3 scripts/modern_stack_dogfood.py \
  --connection /path/to/visuals-ipc.json \
  --receipt-dir /path/to/receipt \
  optimizer --recipe gelo.craftax.hosted.v1 --execute
```

Containers is intentionally two commands. `container-prepare` registers the façade,
prepares an immutable rollout id, creates and opens the family visual with the exact
declared stream, then stops. After two-width review and a current `visual.ready`
receipt, `container-start` resumes from `bound-stream.json`. It cannot silently bypass
the visual gate. `--start-retries 2` repeats the exact immutable start for an
idempotency drill; `--reconnect-after-page N` pauses the consumer and resumes polling
from its last durable cursor without preparing or starting again.

Without `--execute`, the driver is read-only and records an honest `BLOCKED` preflight.
Secrets are recursively redacted before every receipt write.

## Verification run

At the time of this document:

- Workshop optimizer MCP test: passed;
- Workshop dogfood driver: 3 passed;
- Workshop visual/reducer tests: 65 passed, including the 100,000-envelope bound;
- Workshop TypeScript typecheck: passed;
- GELO native Containers tests: 38 passed;
- Containers focused Banking77/Dock tests: 15 passed;
- SFT suite: 38 passed;
- SFT library clippy with warnings denied: passed;
- Workshop broad static/a11y suite: 138 passed, 1 unrelated baseline copy assertion failed (`Plan allowance (ChatGPT)` vs current `Plan allowance`);
- Containers full suite was interrupted after it hung during the pre-existing broad suite/temp cleanup; focused changed-path tests and Ruff are green;
- paid and credential-gated live tests: not run;
- Docker Harbor: not run because the machine daemon was already unhealthy and shared jobs were not disturbed.

Update these counts from command output before merging; do not copy a green count from
another worktree.
