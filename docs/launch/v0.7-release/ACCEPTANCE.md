# v0.7 acceptance (living)

This file is where every lane appends its receipts. Row format is defined once here; do not invent columns. A receipt is a path in this repo (`docs/launch/v0.7-release/…` or `docs/receipts/2026-08-2x/…`) plus a digest; a Codex scratch path or `/tmp` is not a receipt. "Pending" is the only allowed value for an empty cell.

## Exact app (fill when the bytes exist)

- Workshop source: TBD · Product/version: Synth Workshop 0.7.0 · Artifact SHA-256: TBD
- GEPA sidecar: synth-optimizers 0.2.15 (`db040a3d…`) · Eval runtime: TBD (unmanaged today — K2)
- Instance used for packaged acceptance: `~/.synth-desktop/instances/v07/<name>/instance.json` (record `source_revision`; a `-dirty` revision is disqualified)

## Per-lane go/no-go (plan §80–90 minutes)

Mark each lane **go**, **go with documented non-blocking limitation**, or **no-go**. No-go conditions (from the plan): stale lineage or unpinned runtime/image; failed targeted tests without an understood pre-existing cause; an unready Eval target admitted; loss of a terminal manifest / candidate identity / artifact–event linkage; misleading uplift, seed-retention, or failed-work display; a smoke that cannot be reconciled after restart; reliance on an untested V4→V5 Jesterky transition.

| Lane | Targeted automated bar (command · counts · SHA) | Live/packaged smoke receipt (path · digest) | Restart / reconcile receipt | Downstream artifact → inference + Eval receipts | Verdict | Owner | Limitation recorded |
|---|---|---|---|---|---|---|---|
| Shared (lineage, pins, taxonomy, no-zero-imputation) | workshop `cargo test -p synth-desktop --lib --offline optimizers::` **252 passed / 4 ignored** on `7bf65865`; GEPA JS **39/0**; backend training slice **61/0** then **24/0** admission+library on `b84565d6f` | — | — | — | **go with documented non-blocking limitation** | core-dev | GHCR 403; no packaged `.app`; D2/D4 |
| Local MLX SFT / CISPO | synth-mlx-rl `HF_HUB_OFFLINE=1 pytest` **134 passed / 1 failed** (`test_job_has_live_metrics_…` timeout) on `ccb7ebb`; typed capabilities tests in the 252 | pending — `desktop-instance.sh print phase-b` only; Qwen weights present, no `cua-build` | pending | pending | **no-go** (packaged loop not run) | core-dev | K4 unpublished GSM8K eval image; K30 mlx timeout |
| Hosted SFT / CISPO | backend `test_saved_lora_library` + `test_hosted_training_admission` **24 passed** on `b84565d6f`; GSM8K recipe catalogued and start-refused | pending (D2/D4) | pending | pending | **go with documented non-blocking limitation** | L3 / core-dev | rung 0 done; rungs 1–3 gated |
| GEPA | `node --experimental-strip-types --test visuals/tests/gepa_*.test.mjs visuals/tests/optimizer_family.test.mjs` **39/0**; `cargo test -p synth_gepa --lib` **43/1** flake (K12) | pending (packaged) | pending | n/a | **go with documented non-blocking limitation** | L7 / core-dev | no packaged smoke |
| Eval | `python -m synth_optimizers.eval doctor --home /tmp/v07-eval-home --json` → `ready: false` | pending | pending | n/a | **no-go** | L6 | K3/K4 images |

### Row format for lane receipts appended below

```
### <lane> — <YYYY-MM-DD HH:MM local> — <agent>
- SHAs: workshop <sha>, optimizers <sha>/<wheel digest>, synth-mlx-rl <sha>, backend <sha>, beta <sha>, containers image <digest>
- Command: `<exact command>` → <passed>/<failed>/<ignored> (skips named with reason; a skip is not a pass)
- Run id(s): <id> · Recipe: <recipe id> · Instance: <instance.json path>
- Receipts: <repo path> (sha256 <digest>) — manifest, event log, terminal manifest, live-view capture
- Artifact identity retained: <artifact id> · base model <id> · producing run <id> · config digest <sha256>
- Spend: $<amount> against cap $<cap> (provider <id>) · or: none
- Deliberate non-passes / observations: <facts only>
```

## GEPA evidence matrix (L7)

Assert on fixtures or deterministic runs; no case may render unsupported uplift; seed retention reads "no measured improvement".

| Case | Fixture / run | Terminal manifest path | Rendered verdict | Pass |
|---|---|---|---|---|
| accepted proposal | pending | | | |
| all rejected / seed retained | pending | | | |
| failed rollout | pending | | | |
| incomplete coverage | pending | | | |
| cancellation | pending | | | |
| runtime failure | pending | | | |

## Hosted ladder — per-rung receipts (register §4b)

A rung is green only when the **same frozen** backend + optimizers-beta SHAs produce a run with identity, artifacts, reconciliation, and a Workshop-visible receipt. Per-rung receipt = frozen SHAs (backend, beta, containers image digests), run ids, `/training-receipt` JSON, artifact ids + digests, Workshop screenshot of run + artifact, spend.

| Rung | Frozen SHAs (backend · beta · containers) | Admission path | SFT run id · `/training-receipt` path | CISPO run id · `/training-receipt` path | Artifact ids + digests | Workshop capture (path · sha256) | Spend / cap | Probes (`/version`, `/v1/training/capabilities`, `/v1/runtime-identity`) | Verdict | Owner |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 · Reconcile | backend `769fba7e3` (PR #1244; one Alembic head; 58/58 slice; full suite 25 failed known-set / 3127 passed) · beta `ba7ea8d` · containers `9ed2597` | n/a | n/a | n/a | n/a | n/a | none | n/a | **done** (merge + migration check) | L1 |
| 1 · Slot (local, spends Tinker) | pending (baked images, no `docker cp` patches) | validation-only authenticated path (P1-16) — pending | pending | pending | pending | pending | pending (D4) | pending | pending | L3 |
| 1a · CISPO parity | beta `scripts/verify_cispo_parity.py` — one Tinker call | — | — | — | — | — | pending (D4) | — | pending | L3 |
| 2 · Staging (`git staging` → Railway `dev`) | pending | pending | pending | pending | pending | pending | pending | pending | pending (D2) | L1 / L3 |
| 3 · Prod (`git main`; beta deploy) | pending | pending | pending | pending (only if D3 permits) | pending | pending | pending | today: backend `/version`=`128588f`; beta 404/404 | pending (D2) | L1 / L3 |

## Training-contract verification (plan §"v0.7 verification"; register §4)

For one SFT and one CISPO run: native `training.event.v1` stream, mapped `optimizer_event.v1` stream, projected slices (`run.summary/timeline/usage/artifacts/execution`, `training.curves/checkpoints/checkpoint_evaluations/dataset/compute`); ordered replay after a Workshop restart; one terminal result; checkpoint/artifact identity; downstream inference/Eval linkage.

| Run | Native stream path | Mapped stream path | Slices path | Replay after restart | Terminal result | Artifact identity | Downstream linkage | Owner |
|---|---|---|---|---|---|---|---|---|
| SFT | pending | pending | pending | pending | pending | pending | pending | dev agent (blocked on P0-1, P0-6) |
| CISPO | pending | pending | pending | pending | pending | pending | pending | dev agent |

## CUA scenarios (Phase C; not the 90-minute smoke)

| Scenario | Manifest (evals path) | Golden receipt | Triage class of any failure | Disposition | Owner |
|---|---|---|---|---|---|
| CUA-5 Banking77 + HealthBench | pending | pending | | pending (D4 spend) | L4 |
| CUA-1 local MLX GSM8K SFT→CISPO | pending | pending | | pending (P1-8..14) | L4 |
| CUA-2 hosted gpt-oss-20b GSM8K | pending | pending | | pending (rungs 2–3) | L4 |
| CUA-3 GEPA ablations | pending | pending | | pending (D9) | L4 |
| CUA-4 Craftax chain | pending | pending | | pending (after 1–3) | L4 |
| v0.6 W02 / W05 re-verification | dry-run vs installed v0.6.0 | pending | | pending | L4 |

## Appended receipts

(append below using the row format above; newest last)

### core-dev Phase A/B — 2026-08-20 14:20 local — core-dev agent
- SHAs: workshop `56b756dd` (merge of `origin/v0.7` `fb4988af` + A.8 `7f845bb0` + A.9 `7bf65865`); optimizers `4ae4d65` / wheel 0.2.15 `d3c9edd`; synth-mlx-rl `ccb7ebb`; backend `b84565d6f`; beta `aaa262e` (unchanged, not deployed); containers `9ed2597`
- Command: see `TEST_REPORT.md` rows — workshop `optimizers::` 252/0/4; GEPA JS 39/0; Playwright 10/2; eval doctor `ready: false`; mlx-rl 134/1; backend library+admission 24/0
- Run id(s): none · Recipe: `sft.gsm8k.gpt-oss.smoke.v1` catalogued, not started · Instance: `./scripts/desktop-instance.sh print phase-b` only
- Receipts: this file + `TEST_REPORT.md` + `KNOWN_ISSUES.md` + `BETA_DEPLOY.md` (no sha256 of a run manifest — no run)
- Artifact identity retained: none (no SFT job)
- Spend: none
- Deliberate non-passes / observations: D2/D4 not authorized; GHCR 403/404; #52 `base_ref_deleted`; L3 env-flag compatibility hatch still admits CISPO; packed MLX loop not run
