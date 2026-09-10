# v0.7 packaging

## v0.7.4 — as actually built and shipped (2026-08-22)

**This section is the measured record. The v0.7.0 procedure below it is historical and its
Developer ID / notarization steps were NOT used.**

- Artifact (ZIP): `Synth-Workshop-v0.7.4-macOS-arm64-UNNOTARIZED.zip`
  - SHA-256 `782ea3d25323cb7deb08f95d943d7078c18bebdcc7575e95dea14099a9b64e1c`, bytes `121985840`
- Artifact (DMG): `Synth-Workshop-v0.7.4-macOS-arm64-UNNOTARIZED.dmg`
  - SHA-256 `ab1fbf03eebbeb0bf816e474bc645d9ff54a527c08ddb30c0d1f628c319c913d`, bytes `126277366`
- App CDHash `a81e3ad2045f1050a05e166b99c33a5fac075974`; bundle id `com.synth.desktop`; version `0.7.4`
- Main executable SHA-256 `608e6c68f77f326caf1f565ee1cbf14ea1a0ab8d12d1e1db82e54579eaca29ae`
- Workshop source `937a316fcb85e8371faf2bd6f57aceadc4cc1873` / tree `8df5a3e58046fd7fc689580de5000a0ec813e0d4`
- Containers source `e1df8c6ac5629cb11d5bc01bbebc7ffcee0cacbf` / tree `8809da8a335714b837c321c339d03dd9bf7eee1a`
- MLX runtime `synth-mlx-rl` 0.6.0 at `5d6db14330babcff170d2afbb8535de2138385a9`, lock SHA-256 `7f14b704ba9a6c30e6ced5cc88fc2ba6a58a936a9531cfaf168cbb664f83c420`
- Packaged cookbooks: none. `scripts/stage-packaged-cookbooks.sh` is a no-op; cookbooks are not bundled into Workshop.

### Exact commands used

```sh
# 1. build (clean worktree, MLX checkout supplied explicitly)
cd <workshop-worktree>
SYNTH_MLX_RL_PROJECT_ROOT=<synth-mlx-rl worktree> \
  npm run build --workspace @synth/synth-desktop

# 2. stage a copy, then ad-hoc sign it (identity "-", no Keychain)
ditto "apps/synth_desktop/src-tauri/target/release/bundle/macos/Synth Workshop.app" "$OUT/stage/Synth Workshop.app"
codesign --force --deep --options runtime --sign - --identifier com.synth.desktop "$OUT/stage/Synth Workshop.app"
codesign --verify --deep --strict "$OUT/stage/Synth Workshop.app"

# 3. ZIP and DMG
(cd "$OUT/stage" && ditto -c -k --sequesterRsrc --keepParent "Synth Workshop.app" "$OUT/<zip>")
hdiutil create -volname "Synth Workshop 0.7.4" -srcfolder "$OUT/dmg-root" -ov -format UDZO "$OUT/<dmg>"
```

`scripts/release-artifact.sh` was **not** used: its `stage` and `notarize` commands require
`security find-identity` and `notarytool --keychain-profile`, and this release ran under a standing
constraint forbidding macOS Keychain access.

### Verification performed on the shipped bytes

ZIP extracted and DMG mounted; `codesign --verify --deep --strict` passed on both; CDHash identical
across both round trips and equal to the staged app; both reported `CFBundleShortVersionString`
`0.7.4`; the embedded `runtimes/mlx-rl/manifest.json` reported source revision `5d6db143…` and lock
SHA-256 `7f14b704…`. `spctl --assess --type execute` returns **rejected**, which is the expected and
disclosed result for an unnotarized build.

---

## v0.7.0 procedure (historical)


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
