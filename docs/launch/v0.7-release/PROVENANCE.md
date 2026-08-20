# v0.7 provenance (template — fill from `PROVENANCE.json` when the bytes exist)

The package provenance record uses schema `synth.desktop-release-provenance.v1` and is written by `scripts/release-artifact.sh record` (official) or `candidate-record` (ad-hoc candidate). Every field below was recorded for v0.6.0 (Codex `2026-08-19/im/outputs/workshop-v0.6-release-receipt.md`); v0.7 records the same set plus the optimizers wheel and Eval runtime pin. Placeholders are `TBD`; never write a hex string that was not measured.

## Frozen source heads (per repo)

| Repository | Release-candidate head (`origin/v0.7`, 2026-08-20) | Verified release head | Promotion (PR → `main` SHA) | Tag |
| --- | --- | --- | --- | --- |
| `workshop` | `701b483e390d9024f46f60adc500f58c08d07f2f` | TBD | TBD | `v0.7.0` TBD |
| `backend` | `769fba7e3` | TBD | TBD (staging → main per deploy.yml) | — |
| `frontend` | `132d54a9e5212c9566a2223c2042aabaddbd6cf7` (v0.6 catalog) | TBD | catalog PR TBD (mirror PR 263) | — |
| `synth-ai` | `9fd2d175b2802a056074cfe9b19a7e534bf1ad85` | unchanged for v0.7 | — | — |
| `containers` | `9ed2597` | TBD | TBD | — |
| `optimizers` | `279eaf5b83186e3e2157b12c95f54443d3bfdd3d` (`v0.7`, includes experiment layer PR #44) | wheel cut is `d3c9edd` | TBD | `v0.2.15` = `d3c9edd` |
| `optimizers-beta` | `main` `ba7ea8d` (PR #26) | TBD | deploy only (D2) | — |
| `synth-mlx-rl` | `23ee7c3` | TBD | TBD | — |
| `evals` | `dev` `ee80a748d` | TBD | — | — |

## Desktop artifact

- Workshop tree (`source.workshopTree`): TBD
- Containers source / tree (`source.containersCommit` / `containersTree`; must be a **clean** sibling checkout): TBD
- Bundle ID: `com.synth.desktop`; version: `0.7.0`
- Main executable SHA-256: TBD
- Frontend aggregate SHA-256: TBD
- Asset name: `Synth-Workshop-v0.7.0-macOS-arm64-UNNOTARIZED.zip` (candidate path) or `Synth-Workshop-v0.7.0-macOS-arm64.zip` (Developer ID path)
- ZIP bytes: TBD; ZIP SHA-256: TBD (must match GitHub's uploaded-asset digest and the public website copy)
- CDHash before and after ZIP round trip: TBD / TBD (must be equal)
- Signing: `ad hoc` + hardened runtime (D8 default) **or** `Developer ID Application: …`; notarized / stapled: `no` / `no` unless D8 flips
- Gatekeeper assessment: `rejected` (expected when unnotarized) / `source=Notarized Developer ID`
- Staged app launched before record: `no`
- `releaseId`: `workshop-v0.7.0-<first 16 hex of ZIP sha>`
- Provenance asset: `Synth-Workshop-v0.7.0-PROVENANCE.json`; public copy SHA-256: TBD
- Public download URL and `PROVENANCE.json` URL under `https://www.usesynth.ai/releases/v0.7.0/`: TBD

## Runtimes and images pinned into the build

| Runtime | Pin | Digest | Where enforced |
| --- | --- | --- | --- |
| GEPA sidecar `synth-optimizers` | `0.2.15` | wheel `synth_optimizers-0.2.15-cp311-abi3-macosx_11_0_arm64.whl` sha256 `db040a3d9587c64b7bee1bc71c601d27cb9725a8d4480ef52b22706a70645a57`; sdist sha256 `2f29829c23d779f30983917593c0b8a3a1528c3d160014c5a3f52f389d88acf0` (PyPI, 2026-08-20 16:09Z) | `contract/runtimes.rs` `OPTIMIZERS` |
| Eval runtime `synth-optimizers[eval]` | **unmanaged today** (`runtimes.rs` `EVAL`); target `0.2.15` with manifest + digest + installer (PR #49) | TBD | `contract/runtimes.rs` `EVAL` |
| Craftax eval target | `ghcr.io/synth-laboratories/workshop-craftax-eval-target` | `sha256:02b076f8…` (full digest TBD from the catalog TOML; not anonymously pullable — K3) | `synth_optimizers/eval/catalog/*.toml` |
| GameBench target | containers `1b2736295` | `sha256:3065156f…` (full digest TBD) | eval catalog |
| GSM8K eval target | `ghcr.io/synth-laboratories/workshop-gsm8k-eval-target` | TBD (unpublished — K4) | eval catalog |
| Local MLX backend | synth-mlx-rl `23ee7c3` | — | sidecar probe |
| Packaged cookbooks | `synth-cookbooks-public` HEAD used by `scripts/stage-packaged-cookbooks.sh` | TBD | build-time staging |

## Deploy mechanisms (hosted lane)

- **backend** — `.github/workflows/deploy.yml` (on `origin/v0.7`): push to git `staging` → Railway environment `dev` (the product staging fleet); push to git `main` → production fleet. `dev` deploys nowhere. Railway's own git triggers were deleted 2026-08-11; the workflow is the single deploy writer (`railway up` with `RAILWAY_TOKEN_STAGING` / `RAILWAY_TOKEN_PRODUCTION`). The workflow that runs is the one on the pushed branch. Alembic runs at container boot. CI is otherwise suspended and there is no branch protection — the push is the release.
- **optimizers-beta** — observed on `origin/main` `ba7ea8d` (2026-08-20): the repo has **no Dockerfile and no GitHub workflow**; it has `railway.toml` (`builder = "RAILPACK"`, `startCommand = "./bin/optimizers-beta serve"`, `healthcheckPath = "/healthz"`, `restartPolicyType = "ON_FAILURE"`) and `railpack.json` (Rust provider, Python 3.13 installed to `.python` for `requirements-sft.txt`, `SYNTH_SFT_PYTHON` pinned). This contradicts the register's "only `Dockerfile`/`Dockerfile.local`" note; the register should be corrected.
  **TODO (owner: L1, then L3 for rung 1–3 use):** confirm and record here (a) the Railway project / service / environment that backs `optimizers-beta-prod`, (b) what triggers a deploy — Railway's GitHub integration on `main`, a manual `railway up`, or something else — and (c) whether prod is currently at `aaa262e`, `ba7ea8d`, or older (the `/v1/training/capabilities` 404 says pre-CISPO). No deploy without D2.
- **frontend** — Vercel project `frontend` (team `synth-ff365c23`); catalog PR to `main` publishes the artifact and binds the SHA-256 (`src/lib/desktopRelease.ts`); `SYNTH_DESKTOP_STABLE_VERSION` selects the stable line at deploy time.
- **optimizers (PyPI)** — annotated tag `vX.Y.Z` on the merge triggers `.github/workflows/publish-pypi.yml`, which validates tag == `pyproject.toml` version and publishes.
- **containers / eval targets** — GHCR images published by digest from optimizers CI (PRs #35/#36 "make Craftax target public"); the make-public step is the failing piece (K3).
