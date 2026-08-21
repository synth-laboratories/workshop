# v0.7 rollback procedure

v0.7.0 will be a manual-update release, as v0.4–v0.6 were. The last approved public line is **v0.6.0**: artifact `frontend/public/releases/v0.6.0/Synth-Workshop-v0.6.0-macOS-arm64-UNNOTARIZED.zip` (SHA-256 `8c47d773a25c80c91468e9b4a1c3a3a391d027cb073631500b15c83b11fefe56`, 23,245,198 bytes) with `PROVENANCE.json` beside it, catalog entry in `frontend/src/lib/desktopRelease.ts`, tag `v0.6.0` at workshop `58c99e2f`. Rollback therefore has three separate paths; the hosted lane adds a fourth.

## 1. Catalog withdrawal (desktop)

Use when distribution must stop but installed copies do not need replacing.

1. In `frontend`, set the stable line back to `0.6.0`: either the deploy-time `SYNTH_DESKTOP_STABLE_VERSION=0.6.0` on the Vercel project (team `synth-ff365c23`, project `frontend`; always pass `--scope`) or revert `DEFAULT_DESKTOP_STABLE_VERSION` in `src/lib/desktopRelease.ts` and ship through the normal branch → `dev` → `main` path.
2. Verify `https://www.usesynth.ai/releases/stable/latest.json` reports `0.6.0` and `/download` no longer advertises v0.7.0.
3. Keep the `v0.7.0` Git tag and GitHub release for auditability. If the asset itself is unsafe, mark the GitHub release unavailable and remove `public/releases/v0.7.0/` from the website, preserving the hash and provenance in this directory.
4. Verify the withdrawn v0.7.0 URL no longer returns a downloadable artifact and that the v0.6.0 URL and SHA-256 still match (`scripts/workshop-release/verify-artifact.sh --catalog 0.6.0 <zip>` in `frontend`).

## 2. Binary replacement (desktop)

Use when installed v0.7.0 copies require a corrective build.

1. Revert the defective change on a new reviewed branch from the released trunk.
2. Increment the application version (`0.7.1`); never move or recreate the `v0.7.0` tag.
3. Repeat `PACKAGE.md` end to end: clean pinned build, signature, ZIP round trip, checksum, isolated install, provider, restart, and public-download gates.
4. Promote to `main`, create the new immutable tag/release, then update the public catalog (add `0.7.1` to `DESKTOP_STABLE_VERSIONS`, bind artifact URL + SHA-256, flip the default).
5. Verify public manifest, artifact byte count, SHA-256, and a clean install before announcing.

Because updates are manual, the response message must direct affected users to the replacement download. A lower version cannot safely serve as an in-app downgrade.

## 3. Sidecar / Eval runtime pin

The Workshop binary pins the GEPA sidecar at `synth-optimizers` **0.2.15** (`contract/runtimes.rs` `OPTIMIZERS.official` / `min_supported`). A defective 0.2.15 cannot be rolled back by yanking PyPI alone: the installed app would then fail the handshake. The path is a 0.2.16 cut (optimizers `RELEASE.md`; tag push triggers `publish-pypi.yml`) plus a Workshop `0.7.1` with the pin moved — i.e. path 2. If PR #49 lands and the `EVAL` contract is provisioned by Workshop, the same applies to the Eval runtime pin.

## 4. Hosted lane (backend, optimizers-beta)

The git push **is** the deploy (no CI gates, no branch protection):

- **backend**: `git staging` → Railway environment `dev`; `git main` → production (`.github/workflows/deploy.yml`). Roll back by pushing the previous known-good SHA to the rung's branch (`git push origin <sha>:main`); Alembic runs at container boot, so a rollback across a migration needs the down-migration verified first and one head asserted from a clean `__pycache__`.
- **optimizers-beta**: deployed from `main` through Railway (`railway.toml`, RAILPACK builder, start `./bin/optimizers-beta serve`, healthcheck `/healthz`). Roll back via the Railway dashboard "redeploy previous deployment" or by pushing the previous SHA to `main`. See `PROVENANCE.md` §Deploy mechanisms for what is still unconfirmed about the trigger.
- After either rollback re-probe `/version` (backend) and `/v1/training/capabilities` + `/v1/runtime-identity` (beta) and record them here.

Nothing in this file is executed until the v0.7.0 bytes exist; until then the published line is v0.6.0 and no rollback is needed.
