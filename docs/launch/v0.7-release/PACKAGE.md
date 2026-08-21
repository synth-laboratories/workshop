# v0.7 package (procedure; facts filled when the bytes exist)

- Product: Synth Workshop — Version: `0.7.0` — Channel: stable (manual update) — Architecture: macOS arm64
- Signing / notarization: per D8. Default: ad-hoc + hardened runtime, **not** notarized (the `candidate-*` path below). Developer ID + notarization uses the official path.
- Artifact: TBD — ZIP SHA-256: TBD — ZIP bytes: TBD — App CDHash: TBD
- Workshop source: TBD — Containers source: TBD

## Build prerequisites (a fresh worktree fails without these)

See `docs/launch/README.md` §Fresh-worktree build prerequisites: `npm ci --ignore-scripts`, `scripts/stage-packaged-cookbooks.sh`, `scripts/build-computer-use-helper.sh ensure-dev` (or a Developer ID `all`), and no GNU `timeout` on macOS.

## What `scripts/release-artifact.sh` requires

Read from the script on `origin/v0.7`; the script dies on each of these.

1. **Clean tree.** `git status --porcelain --untracked-files=all` must be empty and there must be no staged or unstaged drift (`require_clean_source`). Commit or move everything first — including generated resources.
2. **Resource hygiene.** `visuals/{families,chrome,components,runtime,ambient.d.ts,package.json,tsconfig.json}` must be tracked and contain no ignored files; `visuals/instances` must be a directory if present.
3. **Sibling containers checkout.** `record` reads `SYNTH_CONTAINERS_ROOT` (default `../containers` next to the workshop root) and dies if it is missing or dirty; its commit and tree go into `PROVENANCE.json`.
4. **Signing identity.** Official path: `SYNTH_RELEASE_SIGN_IDENTITY` must be a `Developer ID Application:` identity present in the keychain, and `SYNTH_RELEASE_NOTARY_PROFILE` a `notarytool` profile. Candidate path: `SYNTH_CANDIDATE_SIGN_IDENTITY` (any local codesigning identity; v0.6.0 used ad hoc). `SYNTH_CANDIDATE_INSTALL_APP` defaults to `/Applications/Synth Workshop Candidate.app`.
5. **Output root.** `SYNTH_RELEASE_ROOT` or `${TMPDIR}/synth-desktop-v0.7.0-release`; the `stage/` directory must not already exist. Do not leave the only copy of `PROVENANCE.json` in a temp dir — copy it into this folder and to the GitHub release.
6. **Release adapters.** Every adapter in `scripts/mcp-adapters.sh` must exist under `src-tauri/target/release/` after the build.

## Commands

Official (Developer ID, notarized):

```bash
./scripts/release-artifact.sh stage     # clean source → build → copy adapters → sign (hardened runtime, timestamp)
./scripts/release-artifact.sh notarize  # notarytool submit --wait → staple → spctl must say Notarized Developer ID
./scripts/release-artifact.sh record    # PROVENANCE.json: workshop + containers commit/tree, executable + frontend sha256, cdhash
./scripts/release-artifact.sh zip       # ditto ZIP → extract → verify → CDHash equal → zip sha256/bytes + releaseId into PROVENANCE.json
./scripts/release-artifact.sh install   # install only from the verified ZIP (backs up the previous app)
```

Candidate (ad hoc, unnotarized — the v0.6.0 path):

```bash
./scripts/release-artifact.sh candidate-stage
./scripts/release-artifact.sh candidate-record   # notarized=false, distribution=candidate
./scripts/release-artifact.sh candidate-zip
./scripts/release-artifact.sh candidate-install  # never replaces the official app
```

If the candidate bytes are what ships (D8 default), rename/label the asset `…-UNNOTARIZED.zip`, as v0.6.0 did, and say so in the GitHub release notes.

## After the bytes exist

1. Record ZIP SHA-256, bytes, CDHash (before/after round trip), signing state, source SHAs in `PROVENANCE.md` and here.
2. Tag annotated `v0.7.0` on the workshop `main` merge; create the GitHub release with the ZIP and `Synth-Workshop-v0.7.0-PROVENANCE.json` as assets; confirm GitHub's uploaded-asset digest equals the recorded SHA-256.
3. **Frontend catalog PR** (mirror frontend PR 263, merged as `132d54a9`): add `public/releases/v0.7.0/<zip>` and `public/releases/v0.7.0/PROVENANCE.json`; add `0.7.0` to `DESKTOP_STABLE_VERSIONS`, a `DesktopReleaseMetadata` entry with `publicationStatus: "published"`, `signingStatus`/`notarizationStatus` matching reality, `sourceRevision`, `artifactUrl`, `artifactSha256`; flip `DEFAULT_DESKTOP_STABLE_VERSION`; add a changelog entry under `content/changelog/`; extend `scripts/workshop-release/verify-artifact.sh` `--catalog` with the 0.7.0 checksum and update `verify-catalog.ts`; run the release-catalog Vitest files. Merge to `main` (Vercel production deploy), then verify `releases/stable/latest.json`, `/download`, and an HTTP 200 download whose SHA-256 matches.
4. Install the exact round-trip artifact at `/Applications/Synth Workshop.app` and run the installed-artifact acceptance in `ACCEPTANCE.md`.
