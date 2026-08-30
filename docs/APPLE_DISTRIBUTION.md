# Apple distribution runbook

This runbook covers direct distribution outside the Mac App Store. Local and
friend-test builds use `./scripts/build.sh`; public beta and stable builds must
use the stricter release pipeline below.

## One-command local build

From a source checkout:

```bash
./scripts/build.sh
```

The command checks the toolchain, installs JavaScript dependencies when they
are absent, type-checks and builds the renderer, builds the stable macOS app,
ad-hoc signs a temporary copy, and emits these files under `dist/`:

- `Synth-Workshop-v<version>-macOS-<arch>-UNNOTARIZED.zip`
- the matching `.zip.sha256`
- a JSON manifest containing source commit, dirty-tree state, byte count,
  checksum, architecture, signing state, and notarization state

The word `UNNOTARIZED` is intentional. Gatekeeper rejection is expected for
this artifact, so it is not the public beta artifact.

## Apple account setup

Before cutting a public beta, the release operator needs:

1. An active Apple Developer Program membership with the Account Holder role
   available for Developer ID certificate creation.
2. A **Developer ID Application** certificate and its private key. The
   `Developer ID Installer` certificate is only needed if Workshop later ships
   a signed installer package; it is not needed for the current ZIP flow.
3. The registered bundle identifier `com.synth.desktop`, with any capabilities
   used by the production app reflected in its entitlements and provisioning
   setup.
4. A current full Xcode installation selected by `xcode-select`, providing
   `codesign`, `notarytool`, and `stapler`.
5. Notary-service authentication. For automation, prefer a narrowly scoped App
   Store Connect API key held in the release environment, with its Issuer ID,
   Key ID, and private `.p8` file. An Apple Account plus app-specific password
   is the alternative.
6. A deliberate custody and rotation plan for the Developer ID private key,
   notary credential, and the separate future Tauri updater signing key.

Never commit certificates, private keys, app-specific passwords, API keys, or
notary credentials. Merely building Workshop does not require any of them.

## Existing official release pipeline

The repository's `scripts/release-artifact.sh` is the release authority. It
requires a completely clean source tree and verifies resource hygiene,
Developer ID signing, hardened runtime, timestamps, notarization, stapling,
Gatekeeper acceptance, archive round trips, and provenance.

After the release operator has explicitly configured the signing identity and
notary profile, run:

```bash
export SYNTH_RELEASE_SIGN_IDENTITY='Developer ID Application: <legal name> (<team id>)'
export SYNTH_RELEASE_NOTARY_PROFILE='<notarytool profile>'

./scripts/release-artifact.sh stage
./scripts/release-artifact.sh notarize
./scripts/release-artifact.sh record
./scripts/release-artifact.sh zip
```

The pipeline signs with a secure timestamp and hardened runtime, submits a ZIP
with `xcrun notarytool submit --wait`, staples the ticket to the app, validates
the ticket, and requires `spctl` to report `Notarized Developer ID`. It creates
the distributable ZIP only after stapling and then verifies the extracted app
and its CDHash.

Credential setup is intentionally not performed by the build scripts. It is a
one-time operator action and must be explicitly authorized for the release
machine.

## Public beta gate

Do not publish until all of the following are true:

- the source commit and all packaged dependency repositories are pinned and
  clean;
- the archive is Developer ID Application signed, timestamped, notarized, and
  stapled;
- `codesign --verify --deep --strict` passes on the extracted download;
- `spctl --assess --type execute` accepts the extracted download as
  `Notarized Developer ID`;
- the public HTTPS download returns the recorded bytes and SHA-256;
- the beta release manifest names the same version, channel, architecture,
  source revision, URL, byte count, and SHA-256;
- download, first launch, account creation, promotional entitlement, first
  usage, telemetry correlation, sign-out, and sign-in pass against staging;
- upgrade and rollback behavior are tested before enabling automatic updates.

Apple notarization and a Tauri updater signature solve different problems.
Apple signing/notarization establishes macOS trust; the future updater key
authenticates updates inside Workshop. Keep those keys separate.
