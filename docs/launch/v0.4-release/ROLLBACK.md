# v0.4 rollback procedure

v0.4.0 is a manual-update friends release. Rollback therefore has two separate paths.

## Catalog withdrawal

Use this when distribution must stop but installed copies do not require replacement.

1. Set the public stable catalog back to the last approved version (`0.2.0`) and deploy the website through its normal release branch → `dev` → `main` path.
2. Verify the stable manifest and `/download` no longer advertise v0.4.0.
3. Keep the `v0.4.0` Git tag and GitHub release for auditability. If the asset itself is unsafe, mark the release unavailable and remove the public download route while preserving its hash and provenance in this directory.
4. Verify the withdrawn v0.4 URL no longer returns a downloadable artifact and the approved stable URL/hash still match.

## Binary replacement

Use this when installed v0.4.0 copies require a corrective build.

1. Revert the defective source change on a new reviewed release branch from the released trunk.
2. Increment the application version (for example `0.4.1`); do not move or recreate the `v0.4.0` tag.
3. Repeat the clean pinned build, signature, ZIP round trip, checksum, isolated install, provider, restart, and public-download gates.
4. Promote through `dev` to `main`, create the new immutable tag/release, then update the public stable catalog.
5. Verify the public manifest, artifact byte count, SHA-256, and clean installation before announcing the replacement.

Because updates are manual in the friends channel, the response message must direct affected users to the replacement download. A lower version cannot safely serve as an in-app downgrade.
