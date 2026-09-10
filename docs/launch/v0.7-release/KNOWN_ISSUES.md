# v0.7 known issues (living)

## Shipped in v0.7.4 (2026-08-22) — user-facing known issues

These are the issues published with the release, and they match the public catalog
(`frontend/src/lib/desktopRelease.ts`, `"0.7.4"`) and changelog exactly.

| # | Observed fact |
|---|---|
| V74-1 | The package is **ad-hoc signed and not Apple-notarized**. macOS may require Control-click → Open, or System Settings → Privacy & Security → Open Anyway, on first launch. Gatekeeper assesses it as rejected. |
| V74-2 | Lengthy workload training end-to-end lanes were **not rerun** for this patch: ALFWorld, Craftax, HealthBench, Harvey/OpenRouter, and the full Banking77 GEPA and SFT → CISPO replays. They are outstanding follow-up, not passes. |
| V74-3 | Hosted CISPO remains **fail-closed**; the available canary is not admissible, so no hosted CISPO pass is claimed. |
| V74-4 | On a completed GEPA run, the compact card can show a lower rollout count than the detailed evidence panel and the raw run events, which are authoritative. Display defect only — it is not a run failure. Observed as `16 / 24` on a card whose raw terminal evidence and detailed visual both showed `20 / 24`. |
| V74-5 | Updates are manual in v0.7. About can announce a newer version but always routes back to the official download page. |
| V74-6 | Requires macOS 14 or later on Apple silicon. Intel Macs, Windows, and Linux are unsupported. |

Release-engineering limitations (not user-facing, recorded for the next release owner):

- Developer ID signing and Apple notarization are unavailable: every implemented path in
  `scripts/release-artifact.sh` and `scripts/build-computer-use-helper.sh` requires
  `security find-identity` or `notarytool --keychain-profile`, and the standing credential
  constraint forbids macOS Keychain access. A non-Keychain signing/notarization mechanism must be
  provisioned before any release can claim Developer ID or notarized status.
- GitHub Release asset mirroring is unavailable for the same reason. Tag and branches go over SSH.
- `scripts/desktop-instance.sh` reaches a Keychain only on its default dev-signer path;
  `SYNTH_DESKTOP_USE_DEV_SIGNER=0` selects ad-hoc and is the compliant opt-out.

---

## v0.7.0-era register (historical)

Seeded 2026-08-20 from the v0.7 release work register (§0, §2, §3, §4b, §7) and its keeper log. Each entry is an **observed fact**, the register id, and the owner lane. An entry is removed only when a merged PR closes it; closed entries move to the bottom with the closing SHA.

## Open

| # | Observed fact | Register id | Owner |
|---|---|---|---|
| K3 | `ghcr.io/synth-laboratories/workshop-craftax-eval-target` digest `02b076f8…` is pushed but not anonymously pullable. Core-dev 2026-08-20: `gh api …/workshop-craftax-eval-target` → 403 (`read:packages` missing). `eval doctor` on a fresh home: both Craftax recipes unavailable (`target image … is not present locally`). | P0-4 | L1 |
| K4 | `ghcr.io/synth-laboratories/workshop-gsm8k-eval-target` does not exist (API 404). `eval doctor`: `eval.mlx.local-policy.smoke.v1` reason `target image is not published and pinned yet`. Fixture/gamebench unpublished by design. | P1-9 | L2 |
| K5 | Containers `gsm8k_world.py` still loads `openai/gsm8k` with no `revision=`. Workshop #51 pins `openai/gsm8k` `main` train/test in the local MLX recipe; the loopback world has not been re-exercised. | P1-8 (containers remainder) | L2 |
| K8 | Hosted lane is not deployed: prod backend `/version` = `128588f` (v0.6); prod optimizers-beta answers `/v1/training/capabilities` 404 and `/v1/runtime-identity` 404 (pre-CISPO binary). Mechanism written in `BETA_DEPLOY.md`. No rung 1 rebuild this session (would pull/build images). | P0-9, §4b, D2 | L1 / L3 / Josh |
| K9 | Hosted CISPO stays `not_validated`. L3 merged the D3 `block_reason` (#1246) and a validation-grant admission path (#1247: `admission=validation_only` + `X-Synth-Training-Validation-Grant`). Observed contradiction vs register P1-16: deprecated env flags `SYNTH_DEV_SLOT_MANAGED` + `SYNTH_HOSTED_TRAINING_ALLOW_NOT_VALIDATED` **still admit** (`deprecated_slot_env_admission_enabled`); slot compose still sets `SYNTH_DEV_SLOT_MANAGED=1`. | P1-16, D3 | L3 |
| K10 | CISPO identity has never been measured against Tinker: `verify_cispo_parity.py` has not run (one paid call). | P0-10, D4 | L3 / Josh |
| K11 | Banking77 is saturated for gpt-oss-20b: CISPO 32/32 groups tied (`zero_advantage`), and SFT degraded it 0.81 → 0.47. Banking77 receipts are mechanism receipts, not uplift. | D1 | Josh |
| K12 | `synth_gepa` `drop_does_not_delete_heartbeat_owned_by_another_pid` flakes under parallel threads (4/30 on `d3c9edd`). Reproduced once on `4ae4d65` under default `cargo test -p synth_gepa --lib`; solo `--test-threads=1` passed. | keeper log | optimizers owner |
| K13 | Last packaged Craftax GEPA (0.2.14 sidecar) was 0-for-5 with `codex app-server timed out waiting for response`. Not re-run on 0.2.15 this session. | P1-23 | L7 |
| K14 | GEPA Banking77 recipe still injects `BANKING77_POLICY_CONCURRENCY=4` via env. | P1-29 | L7 |
| K15 | v0.6 shipped with **zero CUA gates closed**; W02 / W05 never re-verified. | P1-7 | L4 |
| K16 | CUA harness is still container-rollout-shaped; Playwright specs are fixture-fed. | P1-1..P1-4, P1-6 | L4 |
| K17 | Nine typed agent capabilities exist on `v07/hosted-pure-dev` `7f845bb0` (PR #54). Workshop #52 auto-closed (`base_ref_deleted` when `v07/local-mlx-surface` was deleted after #51). Not on `origin/v0.7` until #54 merges. Silent-download invariant held: MCP still does not name `training_models_download`; MLX child `HF_HUB_OFFLINE=1`. | P1-13 | dev agent |
| K18 | `docs/sft_tinker_base_models.toml` lists `openai/gpt-oss-20b` (32K) and `sft.gsm8k.gpt-oss.smoke.v1` is catalogued as unavailable / start-refused (workshop #54). Backend #1245 adds `training_context_length: 32768` and `GET/POST …/checkpoints/{id}/inference-target|/chat` (409/402, never Tinker). Not on `origin/v0.7` until those PRs merge. | P1-15, P1-18 | L3 / core-dev |
| K19 | `scripts/desktop-instance.sh` only builds v0.7 (`APP_VERSION=0.7.0`). `-dirty` source disqualifies `assert-identity`. | P0-12 | L1 |
| K20 | MAPO proposer checkpoint `37caf2b` sits on `origin/archive/v0.7-mapo-checkpoint-20260820` with no owner. | §7 | unassigned |
| K21 | Dead code: `spawn_hosted_worker`/`run_hosted_worker` (`hosted_sft.rs`); unused `download_model` in `laguna.rs`; large Rust warning backlog. | §7 | dev agent |
| K22 | `codex/hosted-tinker-training` `569dc7b5` is **not** on `v0.7`. | D11 | dev agent |
| K23 | Stale worktrees `workshop-cua` and `workshop-computer-use` still exist. | §7 | L1 |
| K24 | Distribution is ad-hoc signed and unnotarized (D8). | D8 | Josh |
| K25 | Experiment layer is merged (optimizers PR #44) but not in the 0.2.15 wheel and has no Workshop surface. | P1-19, D9 | experiment-layer owner |
| K26 | Jesterky V5 workflow is disabled. | D10 | deferred |
| K27 | Fresh-worktree builds need `scripts/stage-packaged-cookbooks.sh` and the computer-use helper bundle. | §7 | every lane |
| K28 | Hosted SFT still resolves its service URL from `SYNTH_OPTIMIZERS_SFT_SERVICE_URL` (default `http://127.0.0.1:8878`). | §7 | dev agent |
| K29 | Playwright `optimizer-banking77.spec.ts`: `getByLabel('Artifacts')` strict-mode (2 elements); entry-point spec forbids the string `"Banking77"` but the CISPO card copy is `This Mac · Banking77 CISPO`. 10 passed / 2 failed on `7bf65865`. | P0-7 tail / #45 class | UI / core-dev |
| K30 | `synth-mlx-rl` `test_job_has_live_metrics_terminal_digest_and_durable_handoff` timed out (`job did not become terminal`) under `HF_HUB_OFFLINE=1` on resident Qwen. 134 passed / 1 failed on `ccb7ebb`. | Phase B.1 | mlx-rl owner |

## Closed (retained for history)

| # | Fact | Closed by |
|---|---|---|
| C1 | `desktop-instance.sh` hard-failed on `RELEASE_LINE != v0.6` and pinned `APP_VERSION=0.6.0`, so no v0.7 instance could be built ("the v0.6 trap"). | workshop PR #47 `c589ea9a` (`2e64d4c3`): v0.7 line, `instances/v07/`, 0.2.15 sidecar pin |
| C2 | No `0.2.15` existed anywhere; PyPI latest was 0.2.14. | optimizers PR #42 `d3c9edd`, tag `v0.2.15`, wheel on PyPI 2026-08-20 16:09Z |
| C3 | Workshop `OPTIMIZERS_CONTRACT.official` / `min_supported` were 0.2.14. | workshop PR #47 (`runtimes.rs:142,153` → 0.2.15) |
| C4 | Training-sidecar work existed only as local checkpoint `4694270b`; model root split (`~/.synth-desktop/models/training` vs `instance::state_root()`). | workshop PR #42 `6fcb942e` |
| C5 | synth-mlx-rl `v0.7/real-backend-only` (12 commits) was unpushed; main checkout held UU/AA entries. | synth-mlx-rl PR #2 `23ee7c3` |
| C6 | evals CUA lanes `64366f246` / `2ab5a0891` were on no remote. | evals PR #280 → `dev` `ee80a748` |
| C7 | backend `origin/staging` carried 3 commits not on `v0.7`; promoting would have dropped them. | backend PR #1244 `769fba7e3` (one Alembic head; 58/58 slice) |
| C8 | PPO · GLM card and selector still rendered; SFT/CISPO sat in the optimizer search grid. | workshop PR #47 (P0-7) |
| C9 | `release/v0.6-captain-cua` commits `7dbcca7e` / `02d633d8` were not on `v0.7`. | cherry-picked in PR #42 (`0a7ce836`, `94c90710`) |
| C10 | Hosted CISPO launch did not bind to the retained SFT artifact. | workshop PR #50 `701b483e` |
| C11 | No merged path selected a retained adapter by id for inference/Eval (ambient latest). | workshop #45 `v07/managed-artifacts`, #46 `v07/artifact-inference` (merged into `v0.7`) |
| C12 | Workshop did not provision the Eval runtime (`EVAL` contract unmanaged). | workshop #49 `v07/eval-provisioning` |
| C13 | Local CISPO took rollout URL / warm start from env vars; dropout resume was unguarded. | workshop #51 `v07/local-mlx-surface`; synth-mlx-rl #5 `v07/refuse-dropout-resume` (open) |
| C14 | Training events dropped native identity and skipped MLX sequence gaps silently. | workshop #48 `v07/training-event-adapter` (persisted envelope `optimizer_event.v1`) |
