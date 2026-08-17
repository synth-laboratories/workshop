# Craftax rerun-readiness implementation progress

Branch `agent/craftax-rerun-readiness` off `v0.5/implementation` (`ec4fae7b`).
Sources: `workshop-craftax-rerun-readiness-engineering-handoff.md` +
`workshop-craftax-five-chat-evidence-and-issues.md`
(`~/Documents/Codex/2026-08-16/imp/outputs/`).

Out of scope by instruction: the generation-segment **TPS algorithm**
(`workshop-proper-generation-tps-tracking-handoff.md`) — another engineer owns it.
The usage-accounting half of item 7 (null vs zero, executed vs usage-bearing calls)
is in scope here.

| # | Item | State |
|---|---|---|
| 1 | Campaign contract (10 rollouts, seeds, service-side aggregation) | |
| 2 | Trace V5 render/capture contract | done (workshop side) |
| 2b | Structured errors, never `[object Object]` | done |
| 3 | Container sealing ↔ Workshop trace discoverability | |
| 4 | Terminal, truthful chat lifecycle | |
| 5 | Visual/output ownership | |
| 6 | Live visual certification | done |
| 7 | Truthful usage (TPS excluded) | |
| 8 | Action outcomes + truncation | |
| 9 | Evaluation/replay terminology | |
| 10 | Minimum-width presentation | |
| — | Tool-loop breaker scope | done |

## Notes for whoever picks this up

- `npm run test:visuals` / `test:a11y` are **root** workspace scripts.
- `cargo check --tests` from `apps/synth_desktop/src-tauri` is the fast gate;
  the warning list is pre-existing, diff it before calling anything a regression.
- `node scripts/lint-app-css.mjs` counts `font-size: var(…)` as bare-font-size debt
  (its lookahead backtracks past the whitespace). Style new rules without a
  `font-size` line, or fix the lint separately.
