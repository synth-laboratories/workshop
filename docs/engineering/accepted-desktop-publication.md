# Publish the native-accepted desktop bytes

`Desktop package` builds a clean candidate and uploads its distribution ZIP,
checksum, and manifest. A passing build is not native acceptance. Download that
artifact, verify both hashes, and exercise that exact app on macOS. Record the
run ID, full source SHA, and inner distribution ZIP SHA256 in the release packet.

After review and dev → staging → main promotion, the accepted source must be an
ancestor of main and have the same tree. Create the version tag at that accepted
source SHA, not at a different merge SHA. Tags no longer rebuild or auto-publish.

From main, dispatch **Publish accepted desktop** with the accepted `run_id`,
`source_commit`, and `archive_sha256`. It requires a successful package run from
the correct workflow, checks source ancestry/tree and the existing version tag,
downloads the retained artifact, validates its complete manifest/hash/size/file
set, and creates the GitHub Release without overwriting an existing release.

If the artifact expires, build and accept a new candidate; do not substitute
locally reconstructed bytes under the old receipt. Keep public download/catalog
links held until the release upload and public checksum have been verified.

The app remains ad-hoc signed and non-notarized. `scripts/build.sh` verifies the
trace-import helper can execute, as well as the app's strict signature and
browser-return registration. It does not grant Keychain access or notarization.
Source-build users continue to use `scripts/install.sh` and
`scripts/workshop.sh build-and-run` without Apple Developer credentials.
