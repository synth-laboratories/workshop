# v0.7.4 post-release verification

Supersedes the v0.7.0 skeleton. v0.7.0 was never published; the published v0.7
line is v0.7.1 → v0.7.2 → v0.7.3 → **v0.7.4** (this document).

Verified: 2026-08-22.

## Released identity

- Workshop commit `937a316fcb85e8371faf2bd6f57aceadc4cc1873`; tree `8df5a3e58046fd7fc689580de5000a0ec813e0d4`.
- Annotated tag `v0.7.4`.
- Containers commit `e1df8c6ac5629cb11d5bc01bbebc7ffcee0cacbf`; tree `8809da8a335714b837c321c339d03dd9bf7eee1a`.
- MLX runtime commit `5d6db14330babcff170d2afbb8535de2138385a9` (`synth-mlx-rl` 0.6.0), embedded and
  verified against the packaged wheelhouse manifest (`lockSha256 7f14b704…`).

## Public release

- Download page `https://www.usesynth.ai/download` — advertises 0.7.4.
- Stable manifest `https://www.usesynth.ai/releases/stable/latest.json` — version `0.7.4`.
- ZIP `https://www.usesynth.ai/releases/v0.7.4/Synth-Workshop-v0.7.4-macOS-arm64-UNNOTARIZED.zip`
  — bytes `121985840`, SHA-256 `782ea3d25323cb7deb08f95d943d7078c18bebdcc7575e95dea14099a9b64e1c`.
- DMG `https://www.usesynth.ai/releases/v0.7.4/Synth-Workshop-v0.7.4-macOS-arm64-UNNOTARIZED.dmg`
  — bytes `126277366`, SHA-256 `ab1fbf03eebbeb0bf816e474bc645d9ff54a527c08ddb30c0d1f628c319c913d`.
- The v0.7.3 URLs stay live for rollback.

## Distribution posture

- Signature: **ad-hoc** (`codesign --sign -`), identifier `com.synth.desktop`, no Team identifier,
  CDHash `a81e3ad2045f1050a05e166b99c33a5fac075974` (stable across ZIP and DMG round trips).
- Notarized / stapled: **no**. Gatekeeper assessment: rejected, as expected for an unnotarized build.
- Developer ID signing and Apple notarization were **not** performed. The only implemented paths
  (`scripts/release-artifact.sh stage|notarize`) require `security find-identity` and
  `notarytool --keychain-profile`; this release ran under a standing constraint forbidding any
  macOS Keychain access, so those paths were never invoked. No Keychain facility was opened.
- GitHub Release asset mirroring was **not** performed for the same reason. Source branches and the
  annotated tag are pushed over SSH.

## Deferred — not evidence for this release

Lengthy workload training end-to-end lanes were deliberately deferred by release-owner direction:
ALFWorld, Craftax, HealthBench / hosted `cispo.slime.v1`, Harvey/OpenRouter, and the full Banking77
GEPA and SFT → CISPO training replays were **not** rerun against this candidate. Prior evidence is
retained and stays labelled historical/diagnostic.

Packaged Computer Use acceptance and the packaged lifecycle matrix (optimizer sidecar
orphan/reconciliation, capability approval recovery, installed-app visual QA, report export,
Laguna/generation telemetry, rerunnable candidates) were stopped by release-owner direction before
this ship. The full deferred matrix is recorded in
`Codex/2026-08-21/re/outputs/v0.7.4-engineering-release-handoff.md`.

Hosted CISPO remains **fail-closed**: no admissible `cispo.slime.v1` canary receipt exists.

## Branch continuation

The `v0.7.4` tag stays bound to the released bytes; post-release docs never retag or alter the artifact.
