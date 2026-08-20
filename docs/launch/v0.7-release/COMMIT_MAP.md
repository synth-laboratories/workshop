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
