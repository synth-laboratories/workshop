# Craftax rerun-readiness implementation progress

Branch `agent/craftax-rerun-readiness` off `v0.5/implementation` (`ec4fae7b`).
Sources: `workshop-craftax-rerun-readiness-engineering-handoff.md` +
`workshop-craftax-five-chat-evidence-and-issues.md`
(`~/Documents/Codex/2026-08-16/imp/outputs/`).

Out of scope by instruction: the generation-segment **TPS algorithm**
(`workshop-proper-generation-tps-tracking-handoff.md`) — another engineer owns it.
The usage-accounting half of item 7 is in scope and done.

| # | Item | State |
|---|---|---|
| 1 | Campaign contract (10 rollouts, seeds, service-side aggregation) | done |
| 2 | Trace V5 render/capture contract | done |
| 2b | Structured errors, never `[object Object]` | done |
| 3 | Container sealing ↔ Workshop trace discoverability | done (see gap below) |
| 4 | Terminal, truthful chat lifecycle | partly — see below |
| 5 | Visual/output ownership | done |
| 6 | Live visual certification | done |
| 7 | Truthful usage (TPS excluded by instruction) | done |
| 8 | Action outcomes + truncation | done |
| 9 | Evaluation/replay terminology | done |
| 10 | Minimum-width presentation | done |
| — | Tool-loop breaker scope | done |

Other repos:

- `containers` → `agent/rollout-seal-discoverability` (worktree
  `imp/work/containers-seal-discoverability`): terminal rollout records announce
  their sealed trace. Full suite green (307 passed, 8 skipped).
- `gamebench` → `agent/craftax-action-outcomes` (worktree
  `imp/work/gamebench-action-outcomes`): action effects, truncation accounting,
  usage nullability, success semantics.

## What is not finished

**Item 4 is shared with another lane.** `agent/crash-recovery-stale-working`
(commit `a573d07a`, same base) owns owner-epoch leases, startup reconciliation,
and liveness-gated presence. This branch does *not* duplicate that. What it adds
is the v0.4 P0 that lane does not cover and that `v0.5/implementation` still had
intact: `CodexManager::start` rebinding an attachment by calling `close()`, which
writes the terminal durable status `closed`, after which the next turn's
`Running` transition was refused and silently swallowed. Both branches must land
for item 4 to be complete; they touch `session/` and `storage/migrations.rs` in
overlapping-but-additive ways (this branch adds migration 20, so does that one —
renumber on merge).

**Trace import for the capture-supervisor lane is unverified.** The import path
tries a bundle archive first (`/rollouts/{id}/trace/bundle`), then the lite seal
(`/rollouts/{id}/trace`). Only the second exists in the Containers platform
today, and a lite seal cannot be projected into the inspector — the import says
so honestly (`inspectable: false`). The Craftax capture lane seals through
`synth_containers.tracing.capture`, whose bundle lives on the container host; a
route that serves that bundle is the remaining piece, and it could not be
verified here without the running system.

**Livestream frame-timing evidence (handoff §"Livestream acceptance test") is not
implemented.** The rendered observation carries transport state, counts, and
`observedAt`, but not emission→receipt→render timestamps or p50/p95 latency.

## Notes for whoever picks this up

- `npm run test:visuals` / `test:a11y` are **root** workspace scripts.
- `cargo check --tests` from `apps/synth_desktop/src-tauri` is the fast gate; the
  warning list is pre-existing, diff it before calling anything a regression.
- `node scripts/lint-app-css.mjs` counts `font-size: var(…)` as bare-font-size
  debt (its lookahead backtracks past the whitespace). Style new rules without a
  `font-size` line, or fix the lint separately.
- A bombadil spec that never reaches the state it claims to test passes
  everything in it. The first draft of `minimum-width-replay.spec.ts` clicked a
  trigger that opens the Outputs shelf, not the artifact pane, and went green
  while proving nothing. Every such spec needs an `eventually` that asserts the
  state was actually reached.
- The Craftax container module cannot be imported without its pinned
  `synth-containers` build, which is why action parsing moved to
  `containers/react/action_parsing.py` (no imports) and its tests run anywhere.
  `test_usage_accounting.py` still `importorskip`s the container module.
