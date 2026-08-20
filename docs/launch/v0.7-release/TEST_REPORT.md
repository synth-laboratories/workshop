# v0.7 test report (stub — rows owned by the integrator)

The Workshop stack integrator writes the rows at the end of the v0.7 merge train. This stub fixes the format only. Every row is a command that ran on a named SHA with counts; a compile-only pass, a skipped suite, or a count without a command is not a row.

## Row format

| Repo | SHA | Command (exact) | Passed | Failed | Skipped/ignored (reason) | Notes (pre-existing failures by name, flakes with measured rate) |
|---|---|---|---|---|---|---|

## Passed

Measured 2026-08-20 on this Mac, core-dev worktrees under `~/Documents/GitHub/worktrees/v07-core-dev/`. No cloud, no Tinker spend, no GHCR pull, no model download (`HF_HUB_OFFLINE=1` on the mlx-rl run).

| Repo | SHA | Command (exact) | Passed | Failed | Skipped/ignored (reason) | Notes |
|---|---|---|---|---|---|---|
| workshop | `7bf65865` | `cargo test -p synth-desktop --lib --offline optimizers:: -- --test-threads=8` | 252 | 0 | 4 ignored (pre-existing `#[ignore]` dispatch / live-service tests) | Run before merging UI #43 into this branch. |
| workshop | `7bf65865` | `cargo test -p synth-desktop --lib --offline hosted_sft` | 16 | 0 | 0 | Includes `gsm8k_gpt_oss_recipe_is_catalogued_and_spend_gated`. |
| workshop | `7bf65865` | `cargo test -p synth-desktop --lib --offline tinker_catalog` | 1 | 0 | 0 | `openai/gpt-oss-20b` resolves; default remains Nemotron Lightning. |
| workshop | `7bf65865` | `cargo test -p synth-desktop --bin synth-optimizers-mcp --offline` | 14 | 0 | 0 | Recipe enum contains `sft.gsm8k.gpt-oss.smoke.v1`. |
| workshop | `7bf65865` | `node --experimental-strip-types --test visuals/tests/gepa_*.test.mjs visuals/tests/optimizer_family.test.mjs` | 39 | 0 | 0 | |
| workshop | `7bf65865` | `npx playwright test --config apps/synth_desktop/playwright.config.ts apps/synth_desktop/tests/playwright/optimizer-plugin-mcp.spec.ts apps/synth_desktop/tests/playwright/optimizer-banking77.spec.ts --workers=2` | 10 | 2 | 0 | Failures named below. Fixture-fed; no paid recipes. |
| workshop | `7bf65865` | `./scripts/desktop-instance.sh print phase-b` | contract printed | — | — | `releaseLine=v0.7`, `appVersion=0.7.0`. No `.app` built. `sourceRevision` reported `7bf65865ed18-dirty` because `projectEvents.ts` was dirty in the worktree (not committed). |
| optimizers | `4ae4d65` | `uv run --no-sync python -m pytest tests/test_eval_*.py tests/test_gepa_config_translation.py tests/test_g1_fail_closed.py -q` | 77 | 0 | 0 | |
| optimizers | `4ae4d65` | `cargo test -p synth_gepa --lib` | 43 | 1 | 0 | Failure is K12: `drop_does_not_delete_heartbeat_owned_by_another_pid` under default threads. Solo `--test-threads=1` retry **passed**. |
| backend | `df03d1dd7` then `b84565d6f` | `uv run python -m pytest -q tests/units/test_run_algorithm_kinds.py tests/units/test_training_contracts.py tests/units/test_saved_lora_library.py tests/units/test_memory_bounded_artifact_paths.py` | 61 | 0 | 0 | Slice on the pre-rebase SHA. After rebase onto L3: `test_saved_lora_library.py` **11 passed**; with `test_hosted_training_admission.py` **24 passed** on `b84565d6f`. |
| synth-mlx-rl | `ccb7ebb` | `HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 .venv/bin/python -m pytest -q` after `uv pip install -e '.[mlx]'` | 134 | 1 | 0 | Resident Qwen3.5-0.8B in HF cache; Hub offline. Counted from progress bar (135 collected). Failure named below. |
| evals | `cda044cbc` / `origin/dev` `ee80a748` | not re-run this session | — | — | — | #280 already merged (36/36 recorded by L5). |

## Observations

- Playwright `optimizer-banking77.spec.ts:136` — `getByLabel('Artifacts')` strict-mode violation (2 elements). Same class of defect called out on #45.
- Playwright `optimizer-banking77.spec.ts:192` — page must not contain `"Banking77"`, but the CISPO training card copy is `This Mac · Banking77 CISPO`.
- `synth_gepa` heartbeat ownership flake reproduced once under parallel, then passed solo (K12).
- `synth-mlx-rl` `tests/test_service.py::test_job_has_live_metrics_terminal_digest_and_durable_handoff` — `AssertionError: job did not become terminal` after a ~3 min suite. Did not retry; not a download failure.
- `python -m synth_optimizers.eval doctor --home /tmp/v07-eval-home --json` on `4ae4d65`: `ready: false`. Craftax recipes: image digest pinned (`sha256:02b076f8…`) but **not present locally**; GSM8K recipe `eval.mlx.local-policy.smoke.v1`: `target image is not published and pinned yet`. Fixture/gamebench: unpublished by design.
- `gh api /orgs/synth-laboratories/packages/container/workshop-craftax-eval-target` → **403** (`read:packages` missing). `workshop-gsm8k-eval-target` → **404**.
- Qwen3.5-0.8B weights are already at `~/.synth-desktop/models/training/Qwen/Qwen3.5-0.8B` and in the HF hub cache. Packaged SFT→CISPO was **not** started: no `cua-build` `.app`, D4/no-spend, and `eval.mlx.local-policy.smoke.v1` is unpublished.
- Slot compose still sets `SYNTH_DEV_SLOT_MANAGED=1`. L3 on backend `v0.7` (`#1247`) still admits `not_validated` CISPO through those deprecated flags. Register P1-16 asked to replace the env-flag gate; the merged path keeps it as a one-release compatibility hatch. Rung 1 was **not** rebuilt (would pull/build images).
- Workshop #52 (`v07/typed-agent-capabilities`) auto-closed when GitHub deleted its base `v07/local-mlx-surface` after #51 merged (`base_ref_deleted`). A.8 commit `7f845bb0` is on `v07/hosted-pure-dev` / PR #54.

## External acceptance boundaries

- **D2** — no deploys. Exact commands not run: `railway up --service optimizers-beta-prod`; `git push origin main` (backend prod). Mechanism: `docs/launch/v0.7-release/BETA_DEPLOY.md`.
- **D4** — no Tinker spend. Exact commands not run: `uv run python scripts/verify_cispo_parity.py`; start of `sft.gsm8k.gpt-oss.smoke.v1`; `POST /api/v1/optimizers/checkpoints/{id}/chat` with `execute=true`; slot3 bounded hosted SFT/CISPO launches.
- **D8** — unnotarized; no `assert-identity` (would require a clean tree).
- Packaged local-MLX acceptance (register Phase B.3) and training-contract replay (B.4) were not run: they need a `desktop-instance.sh cua-build` `.app` plus a real SFT/CISPO job. Weights exist; the smoke was stopped before launch.
- GEPA on a packaged build (B.5) was not run; JS fixture tests (39) are the GEPA bar this session.
