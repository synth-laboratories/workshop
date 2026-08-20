# v0.7 COMMIT_MAP

Frozen 2026-08-20 during Phase A freeze+push. Base for workshop/optimizers/containers is `origin/v0.7` unless noted. Workshop git cannot create `v0.7/<lane>` refs because `refs/heads/v0.7` already exists; lanes use `v07/<lane>`.

| Repo | Frozen base | Handoff SHA | Branch | Notes |
|---|---|---|---|---|
| workshop | `origin/v0.7` `aabe65da` | this branch | `v07/training-sidecar` | Split of local checkpoint `4694270b`; model-root uses `instance::state_root()`. |
| synth-mlx-rl | `origin/v0.7` `ef0908c` | `30bd8c9` | `v0.7/real-backend-only` | 12 commits ahead of origin/v0.7. Live worktree `worktrees/v07-training-identity/synth-mlx-rl` left untouched (dirty). Main checkout cherry-pick aborted. |
| backend | `origin/v0.7` `e7b844d24` | `b5468f67b` | `v0.7/staging-reconcile` | `ort` merge of `origin/staging` (`20d0e18fd`). Tree identical to `origin/v0.7` (pure history join). Staging-only SHAs `5fe957dd7` / `9e0657753` / `f15a2ee27` are content-equivalent to commits already on v0.7 via main. Migration files: 363; no duplicate revision ids; workshop_usage + report_public migrations present. |
| evals | `origin/dev` `9460149a9` | this branch | `v0.7/workshop-evals` | `origin/dev` already contains the evolved `workshop/` tree from `2ab5a0891`. Ported `64366f246` release-line instance resolution onto current origin/dev. |
| optimizers | `origin/v0.7` `c4e53ff` | pending 0.2.15 cut | `v0.7/cut-0.2.15` | Fresh worktree. Experiment layer excluded (D9). |
| containers | `origin/v0.7` `9ed2597` | unchanged | — | No freeze commit. |
| optimizers-beta | `origin/main` `aaa262e` | unchanged | — | No deploy in this handoff (D2). |
| frontend | `origin/v0.7` `132d54a9` | unchanged | — | Catalog PR is Phase C. |

## D11

- `release/v0.6-captain-cua` unique commits `02d633d8` (runtime family through IPC hydration) and `7dbcca7e` (terminalize failed eval visual projections) are not on `origin/v0.7`. Cherry-pick attempted on the training-sidecar branch; see the commits that follow if they applied.
- `codex/hosted-tinker-training` `569dc7b5` is **not** already on v0.7. It is a parallel hosted model/checkpoint UX (`optimizers/training.rs`, `models.rs`, `cloud.rs`) that overlaps `OptimizersPage.tsx` with the sidecar land. Not merged. Unique files are not dropped from history; they remain on `origin/codex/hosted-tinker-training` for a later replay if the sidecar path is insufficient.

## Do-not-touch worktrees (live as of 2026-08-20 11:48)

- `~/Documents/GitHub/optimizers-v07-visuals`
- `~/Documents/GitHub/worktrees/workshop-v07`
- `~/Documents/GitHub/worktrees/v07-training-identity/synth-mlx-rl`

## Integration state 2026-08-20 17:30Z (L5 extension; L1's freeze table above is unchanged)

Merged PRs and the resulting `v0.7` (or `dev`/`main`) heads. Keeper-log and `gh` are the sources; re-fetch before trusting.

| Repo | Branch | Head | Merged PRs (newest first) | Open lanes |
|---|---|---|---|---|
| workshop | `v0.7` | `701b483e` | #50 `v07/hosted-cispo-sft-binding` → `701b483e`; #47 `v07/cheap-gates` → `c589ea9a` (supersedes #44, auto-closed when its base was deleted); #42 `v07/training-sidecar` → `6fcb942e`; #41 `feature/v07-gepa-visual` → `aabe65da`; #40 `agent/v07-cispo-healthbench-proof` → `ce743867` | stack: #45 `v07/managed-artifacts` (base `v0.7`) → #46 `v07/artifact-inference` → #48 `v07/training-event-adapter` → #49 `v07/eval-provisioning` → #51 `v07/local-mlx-surface` → #52 `v07/typed-agent-capabilities`; #43 `codex/v07-ui-training-artifacts` (UI lane, base `v0.7`); this docs PR `v07/release-docs` |
| optimizers | `v0.7` | `279eaf5` | #44 `v07/experiment-layer-pr` → `279eaf5` (experiment layer + `synth.correlation.v1`; **not in the 0.2.15 wheel**); #42 `codex/v0.2.15-release` → `d3c9edd` | #43 `v07/policy-snapshot-registrar` (`PolicySnapshotRegistrar` for `mlx-lora.v1`, pairs with workshop #46) |
| optimizers tag | `v0.2.15` | `d3c9edd` | PyPI 2026-08-20 16:09Z: wheel `synth_optimizers-0.2.15-cp311-abi3-macosx_11_0_arm64.whl` sha256 `db040a3d9587c64b7bee1bc71c601d27cb9725a8d4480ef52b22706a70645a57`; sdist sha256 `2f29829c23d779f30983917593c0b8a3a1528c3d160014c5a3f52f389d88acf0`. Contains `2ed30aa` (verified ancestor). | — |
| backend | `v0.7` | `769fba7e3` | #1244 `v07/staging-reconcile` (rung 0; one Alembic head; 58/58 slice; full suite 25 failed known-set / 3127 passed); #1243 training-run identity → `e7b844d24` | `staging` `20d0e18fd`, `main` `128588f35` unchanged (no deploy, D2) |
| synth-mlx-rl | `v0.7` | `23ee7c3` | #2 `v07/real-backend-only` (132/0/0 offline against resident Qwen; ruff 67) | — |
| evals | `dev` | `ee80a748d` | #280 `v0.7/workshop-evals` (36/36 workshop tests) | L4 harness work |
| containers | `v0.7` | `9ed2597` | — | `agent/lane3-m1-containers-20260814` pushed (v0.8 per rerun design) |
| optimizers-beta | `main` | `ba7ea8d` | #26 `codex/hosted-sft-cispo-lineage` (after `aaa262e`, the SHA the register froze) | deploy only (D2); mechanism in `PROVENANCE.md` |
| synth-dev | `main` | `3a0176aa` | — | `codex/local-slot-cispo-healthbench-canary-20260819` (slot compose for L3) |
| frontend | `main` | `132d54a9` | v0.6 catalog PR 263 | v0.7 catalog PR (Phase C) |

Archived / preserved refs (never merge wholesale): optimizers `archive/v0.7-mapo-checkpoint-20260820` (`37caf2b`), `archive/codex-gepa-v07-evidence-bundle-20260820` (`0aa9ad4b`), `codex/eval-v5-annotation-policy` (`79c6a0d6`); workshop `codex/hosted-tinker-training` (`569dc7b5`, D11 not merged), `codex/v07-local-sft-cispo` (fully represented on `v0.7` via #50).

Merge rule for the rest of the train (register §12): merge-commit into the core branch, delete the head branch so GitHub retargets the stacked PR, never squash a stack, re-verify the stack head after each merge, record counts in `TEST_REPORT.md`.
