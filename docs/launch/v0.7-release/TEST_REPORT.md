# v0.7 test report

Rows written by the Workshop stack integrator at the end of the v0.7 merge train (2026-08-20). Every row is a command that ran on a named SHA with counts; a compile-only pass, a skipped suite, or a count without a command is not a row.

## Row format

| Repo | SHA | Command (exact) | Passed | Failed | Skipped/ignored (reason) | Notes (pre-existing failures by name, flakes with measured rate) |
|---|---|---|---|---|---|---|

## Passed

Final `origin/v0.7` tip after the train: `2a77535a` (merge of #54). Commands ran from a fresh worktree of that SHA on this Mac (macOS, Apple Silicon; `npm ci --ignore-scripts`; packaged cookbooks staged; Computer Use helper bundle copied).

| Repo | SHA | Command (exact) | Passed | Failed | Skipped/ignored (reason) | Notes |
|---|---|---|---|---|---|---|
| workshop | 2a77535a | `cd apps/synth_desktop && npm run typecheck` | clean | 0 | — | `tsc --noEmit` |
| workshop | 2a77535a | `node --test apps/synth_desktop/tests/*.test.mjs` | 426 | 0 | 0 | baseline 426 at 701b483e held through the train |
| workshop | 2a77535a | `cd apps/synth_desktop && PLAYWRIGHT_WORKERS=4 npx playwright test tests/playwright/optimizer-*.spec.ts tests/playwright/training-*.spec.ts` | 18 | 0 | 0 | 12 optimizer specs + 6 `training-workspace.spec.ts` (added by #43) |
| workshop | 2a77535a | `node apps/synth_desktop/scripts/lint-app-css.mjs` | clean | 0 | — | `app.css` style-literal debt did not increase |
| workshop | 2a77535a | `cd apps/synth_desktop/src-tauri && cargo test -p synth-desktop --lib optimizers:: --no-fail-fast` | 252 | 0 | 4 ignored: `cispo::tests::local_cispo_dispatch_needs_synth_mlx_rl`, `mlx_sft::tests::local_sft_dispatch_needs_synth_mlx_rl` (need synth-mlx-rl + managed Qwen weights), `recipes::tests::paid_craftax_smoke_reaches_terminal_through_the_real_sidecar`, `recipes::tests::paid_dual_banking77_luna_sol_receipt` (paid; D4) | was 231 at #45's head |
| workshop | 2a77535a | `cd apps/synth_desktop/src-tauri && cargo test -p synth-desktop --lib contract::specta::` | 1 | 0 | 1 ignored: `regenerate_protocol_bindings` (writes `generated/protocol.ts`; run explicitly) | binding count 241 (240 → 241 at #46) |
| workshop | 2a77535a | `cd apps/synth_desktop/src-tauri && cargo test -p synth-desktop --lib -- training_artifacts training_adapter eval_runtime runtimes::tests mlx_sft::tests cispo::tests training_models::tests` | 31 | 0 | 2 ignored (the two `local_*_dispatch_needs_synth_mlx_rl` above) | includes the 7 `training_adapter` contract tests (identity retained, gaps fail, replays skipped, terminal mapping explicit) |
| workshop | 2a77535a | `cd apps/synth_desktop/src-tauri && cargo test -p synth-desktop --bin synth-optimizers-mcp` | 14 | 0 | 0 | covers the nine typed capabilities' confirm gating before IPC |
| workshop | 2a77535a | `git diff origin/v0.7...HEAD \| grep '^+' \| grep -E 'unimplemented!\|todo!\|TODO\|dbg!\|console.log\('` | 0 hits | — | — | run at every PR head |

Per-PR heads verified with the same bar before each merge (counts are `optimizers::` passed / Playwright passed; node was 426/426 and typecheck, CSS lint, specta, hygiene green at every head):

| PR | Head verified | Merge commit | optimizers:: | Playwright | Fix commits at the head |
|---|---|---|---|---|---|
| #45 `v07/managed-artifacts` | 1aaf1b04 | 510ce4e8 | 231 | 12 | none |
| #46 `v07/artifact-inference` | 3b426709 | 9b2b7da2 | 234 | 12 | 3b426709 specta regen (240 → 241) |
| #48 `v07/training-event-adapter` | 19f0114f | 10aa6eab | 242 | 12 | 19f0114f parenthesize `projectEvents.ts:2165` |
| #49 `v07/eval-provisioning` | 0144c6cf | 714c6bc1 | 245 | 12 | none |
| #51 `v07/local-mlx-surface` | 85594836 | d99a90fc | 246 | 12 | none |
| #43 `codex/v07-ui-training-artifacts` | 11060c1f | fb4988af | 246 | 18 | 11060c1f workspace row test ids |
| #55 `v07/typed-agent-capabilities` | 4dcb6b99 | 5af338fe | 251 | 18 | 4dcb6b99 `inspect_training_artifact` tool name |
| #54 `v07/hosted-pure-dev` | 8b3b54d7 | 2a77535a | 252 | 18 | none |

## Observations

- `training-workspace.spec.ts` ("retain CUA-1 training receipts") rewrites the PNGs under `docs/receipts/2026-08-20/v0.7-training-ui/` on every run; the committed receipts were restored before each push, so the tree carries the UI lane's originals.
- `OptimizersPage.tsx` still carries four `"ppo"` references (saved-LoRA filter option at the `All algorithms` select, a type union, and two `includes` guards). They pre-date the train (present on d99a90fc before #43) and render one option label `PPO`; left for the owner under D7 (SDK `submit_ppo` removal deferred to v0.8).
- `sft.gsm8k.gpt-oss.smoke.v1` (#54) is catalogued unavailable and refuses `start`; it pins the 32K context but not yet the `openai/gsm8k` dataset constants from `mlx_sft.rs` (#51).
- Not measured here (recorded by the register, not re-run): optimizers `synth_gepa` `service_ownership::tests::drop_does_not_delete_heartbeat_owned_by_another_pid`, 4/30 parallel failures on `d3c9edd`, 0/10 solo (KNOWN_ISSUES K12); backend full suite 25 known failures / 3127 passed at #1244.
- GitHub returned 504 on `gh pr merge` for #45 while the merge had in fact completed; every merge was confirmed by `gh pr view --json state,mergeCommit` before the next step.

## External acceptance boundaries

Not performed in this train, each gated by a decision id:

- Paid runs — the two ignored `recipes::tests::paid_*` acceptances, the Banking77 GEPA live run, the hosted CISPO parity call, and any launch of `sft.gsm8k.gpt-oss.smoke.v1` or the Tinker slot rung (D4).
- Deploys — backend staging/prod promotion and the optimizers-beta beta deploy described in `BETA_DEPLOY.md` (D2).
- Notarization — v0.7 ships unnotarized as v0.6 did unless told otherwise (D8).
- Local MLX acceptance on a packaged `desktop-instance.sh` v0.7 build (Phase B §3) and the training-contract replay receipt (Phase B §4) are Phase B work, not part of this merge train.
