# Synth Workshop v0.7.0 — release notes (living draft)

Status: **draft, not released.** The published stable line is v0.6.0 (`frontend/public/releases/v0.6.0/`). Last revised 2026-08-20 against workshop `origin/v0.7` `701b483e`.

v0.7 is an integration-and-proof release: first-class Hosted SFT/CISPO, Local MLX SFT/CISPO, GEPA, and Eval under one Optimizers sidecar, with pinned runtimes, durable evidence, honest terminal state, and retained acceptance receipts. Source of truth for scope: `v0.7-release-plan.md` and the v0.7 release work register (Codex `2026-08-20/wha/outputs/`).

Vocabulary used below, everywhere:

- **implemented** — the code is merged on the repo's `v0.7` branch with its automated tests green.
- **proven** — a retained receipt (path cited) shows the behaviour on the release build or the frozen SHAs.
- **pending** — no receipt exists today. A pending row is not a release claim.

## Core supported

The four lanes from the plan's "Supported optimizer lanes" table. A lane ships as core only when its **proven** column is filled before the go/no-go review (`ACCEPTANCE.md`).

| Lane | Implemented (merged on `v0.7`) | Proven (receipt path) | Pending |
|---|---|---|---|
| **Hosted SFT / CISPO** | backend training-run identity registry + idempotent checkpoint redelivery (backend PR #1243 `e7b844d24`); staging reconciled into `v0.7` (PR #1244 `769fba7e3`); optimizers-beta `RunKind`/CISPO runtime (PRs #24/#25 → `aaa262e`) and hosted-SFT warm-start lineage (PR #26 → `ba7ea8d`); Workshop training sidecar + capability placements (PR #42 `6fcb942e`), hosted CISPO bound to the retained SFT artifact (PR #50 `701b483e`) | HealthBench Tinker RL evidence, `docs/launch/v0.7-cispo-healthbench-canary.md` — proves the hosted training mechanism, **not CISPO** (Tinker native `importance_sampling`, not `cispo.slime.v1`; catalog stays `not_validated`) | hosted ladder rungs 1–3 (`ACCEPTANCE.md` §Hosted ladder); nothing is deployed (prod `/version` = `128588f`, beta `/v1/training/capabilities` 404); CISPO parity never measured (P0-10, D4); training-event adapter (P0-6, PR #48 open); hosted checkpoint sampling (P1-18) |
| **Local MLX SFT / CISPO** | synth-mlx-rl `v0.7` `23ee7c3` (PR #2: real backend only, batching, resume, CISPO backend + rollout sequencer; 132 tests); Workshop sidecar dispatch for local SFT (`mlx_sft.rs` → `sidecar_training::create_and_watch`) and local CISPO (`cispo.rs`), instance-scoped model roots (PR #42); GEPA sidecar pin 0.2.15 and training taxonomy (PR #47 `c589ea9a`) | v0.6 receipt only: resident Qwen MLX SFT, paired held-out 8/8, adapter digest `b62f3393…` (Codex `2026-08-19/im/outputs/workshop-v0.6-release-receipt.md`) | packaged v0.7 local acceptance (bounded SFT → artifact → inference → Eval → CISPO warm start); managed artifact record (P0-2, PR #45 open); artifact-addressed inference/Eval (P0-1, PR #46 open); GSM8K pin + dropout-resume refusal (PR #51 open); typed agent capabilities (P1-13, PR #52 open); the local CISPO recipe still reads `SYNTH_MLX_CISPO_*` env (`cispo.rs:23,84-96`) |
| **GEPA** | optimizer workbench controls (PR #41 `aabe65da`); reducer, terminal sealing, and fixtures on `v0.7` (`cargo test -p synth-desktop --lib optimizers::` 231 passed / 4 ignored at `905ef812`; GEPA JS 39); optimizers `synth_gepa` 45–47 lib tests on `d3c9edd`; minibatch-pool degeneracy guard (`recipes.rs:2029-2041`) | last live GEPA receipt is v0.4 Banking77 (`docs/launch/v0.4-release/ACCEPTANCE.md`); no v0.7 live or packaged GEPA receipt exists | Craftax deterministic smoke + restart/reconciliation + evidence matrix on the packaged build (L7, P1-23); Banking77 bounded live run (D4 spend); 0.2.14-era Craftax app-server timeout to be re-checked on 0.2.15 |
| **Eval** | `synth-optimizers` 0.2.15 published from the plain `c4e53ff` lineage (optimizers PR #42 → `d3c9edd`, tag `v0.2.15`; wheel sha256 `db040a3d…`, see `COMMIT_MAP.md`); contains admission-readiness hardening `2ed30aa` (verified ancestor); Eval runtime tests 60 + 14 on `d3c9edd`; Workshop allowlist of eight `eval.*` recipes (`contract/runtimes.rs` `EVAL.bounded_recipes`) | none on v0.7 | Workshop still does not provision Eval — `EVAL` contract is `official: "unmanaged"`, `provisioned_by_desktop: false` (`runtimes.rs:175-205`; P0-3b, PR #49 open); Craftax target `ghcr.io/synth-laboratories/workshop-craftax-eval-target` not anonymously pullable (P0-4); GSM8K target unpublished (P1-9); one report-only Eval run with manifest, staged candidate identity, trial results, event log, terminal status, live-view receipt |

Shared gates that are **implemented**: backend `v0.7` carries staging history (rung 0 done); Workshop `desktop-instance.sh` builds `v0.7` instances under `~/.synth-desktop/instances/v07/` (PR #47); optimizer taxonomy cleanup — PPO card/selector removed, SFT/CISPO tagged `kind: "training"` and out of the search grid (PR #47; `service.rs:294-295`).

Shared gates that are **pending**: the defining v0.7 flow (train → select retained artifact → inference → Eval with the artifact retained in both receipts) has no merged code path yet (register §0 item 1; PRs #45/#46 open). Under D6 the RC is held for it.

## Opt-in / experimental

Nothing in this section is a release claim. Code presence is not "shipped".

- **Experiment / ablation layer** (`synth_optimizers.experiment`, `synth.correlation.v1` through GEPA, `eval.runtime` + `gepa.cli` adapters, CLI `synth-optimizers experiment factors|plan|aa|run|resume|report`). Merged as its own PR — optimizers PR #44 `279eaf5` on `v0.7`, 2026-08-20 17:15Z — **after** the 0.2.15 cut, so it is **not in the 0.2.15 wheel** (D9; `changelog.log` "Unreleased -- experiment layer"). Verified read-only at `7071088`: 143 pytest, platform 48, `synth_gepa` 47 (`--test-threads=1`). Opt-in CLI only; no Workshop surface (P1-22 has a producer but no spec ingestion or arm/block/claim view). Becomes a release claim only with its own A/A + two-arm receipt; packaging it means a 0.2.16 cut (release call, not made).
- **Jesterky V5 annotations** — disabled. `JesterkyWorkflowConfig` (`src/synth_optimizers/gepa.py:1252`) defaults off; the V5 annotation-policy lane is `origin/codex/eval-v5-annotation-policy` (pushed, unmerged). Not in release claims (D10).
- **MAPO Codex proposer + ASCII system diagrams** — archived, not on `v0.7`: `origin/archive/v0.7-mapo-checkpoint-20260820` (`37caf2b`, 1,351 lines, no owner). P2 per register §7.
- **GEPA-proposer evidence bundle** — `origin/archive/codex-gepa-v07-evidence-bundle-20260820`; cherry-pick source only (P1-21), stale lineage, never merge wholesale.
- **Hosted CISPO catalog** — `openai/gpt-oss-20b` CISPO remains `not_validated` (rev `2026-08-19.v4`). Admission on the slot is an env-flag gate, not the authenticated validation-only path (P1-16, L3). D3 default: keep `not_validated`, reword the stale `block_reason`.

## Deferred to v0.8

Every item `docs/launch/v0.7-scope.md` scheduled that the release plan does not cover gets an explicit line (register §6). Default disposition: **deferred**, except what P0-2 (artifact library) and P1-22 (experiment overview) deliver incidentally.

| `v0.7-scope.md` item | Disposition | Note |
|---|---|---|
| Broader chart interactions (sorting, filtering, linked selection, annotations, view-state polish) | deferred to v0.8 | no v0.7 PR touches it |
| Experiment DAG visualization | deferred to v0.8 | already "deferred 2026-08-19" in the scope doc; no graph models/endpoints/canvases in v0.7 |
| Local public-cookbook catalog + download UX; broader cookbook turnover | deferred to v0.8 | packaged cookbooks are staged at build time by `scripts/stage-packaged-cookbooks.sh` only |
| Rich catalog tags, rename/purge, checkpoint-library polish | deferred to v0.8 | the managed artifact record (P0-2, PR #45) delivers list/detail/provenance, not tags or rename/purge |
| Hosted OHCO and GELO optimizer support | deferred to v0.8 | OHCO is advertised in the startup catalog but never admitted by backend; MAPO is admitted by backend but rejected by beta's parser. `hosted_gelo.rs` on v0.7 is the existing v0.6 Craftax GELO pattern, not a GELO ship |
| Consent-safe analytics / growth instrumentation | deferred to v0.8 | no v0.7 PR |
| Audience-selector polish | deferred to v0.8 | no v0.7 PR |

Also deferred, from the release plan and the rerunnable-candidates design:

- **Portable container-candidate abstraction** (`synth.candidate.v1` + `synth.task-contract.v1` bundle, `/candidates` route, bundle manifest, portable launcher) — v0.8 per `rerunnable-candidates-design.md` §9. v0.7 only completes existing records (artifact id, base model, producing run, config digest) so a finetune/LoRA can be loaded and evaluated; containers `agent/lane3-m1-containers-20260814` (variants API + frozen splits) is pushed but v0.8 by that design.
- **de-GEPA renames** in `synth_optimizer_platform` (`validate_for_gepa` ×14, `gepa_contract` ×9, `verify_gepa_contract` ×4) — register §7.
- **SDK `submit_ppo`/`submit_online_reflexion` removal and `/api/v1/training/runs` alias** — v0.8 (D7 default).
- **Bit-exact resume under LoRA dropout; federated local↔hosted checkpoint store; folding `eval.*` into the sidecar; extra local base models without a measured admission probe** — out of v0.7 per `v0.7-optimizers-runtime.md`.

## Distribution

- macOS arm64, macOS 14+. Signing/notarization per D8: default is **ad-hoc signed, unnotarized**, as v0.6.0, unless Josh decides otherwise. The Computer Use helper cannot ship without a Developer ID (dev instances bypass via `PARENT_REQUIREMENT_ENV`).
- Updates remain manual through `https://www.usesynth.ai/download` and `releases/stable/latest.json`.
- Artifact, SHA-256, CDHash, and provenance land in `PACKAGE.md` / `PROVENANCE.md` when the bytes exist.
