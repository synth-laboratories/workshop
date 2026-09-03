# v0.4 post-release verification

Verified 2026-08-16.

## Public release

- GitHub release: `https://github.com/synth-laboratories/workshop/releases/tag/v0.4.0`
- Download page: `https://www.usesynth.ai/download`
- Stable manifest: `https://www.usesynth.ai/releases/stable/latest.json`
- Public artifact: `https://www.usesynth.ai/releases/v0.4.0/Synth-Desktop-v0.4.0-macOS-arm64-UNNOTARIZED.zip`
- Public artifact result: HTTP 200, `application/zip`, 19,360,702 bytes
- Public artifact SHA-256: `a1f2e882ccc7ac4eeab31ce55b1548a11114cd6b3c10f5290a4e94cecaa114ec`

## Installed-artifact resilience

- ChatGPT subscription, OpenRouter, and managed local Laguna provider smokes passed.
- A forcibly killed Laguna sidecar relaunched on demand and completed the next request.
- The exact installed application relaunched after a forced app-process kill.
- The durable Banking77 GEPA visual reopened with its 140 scored rollouts, heldout score, TPS/elapsed history, candidate/frontier state, and complete proposer trace.
- Duplicate terminal usage, duplicate completion/usage, partial migration recovery, and optimizer event replay deduplication passed focused tests.

## Branch continuation

The Workshop, public website, and cookbook `v0.5/implementation` branches were created from their reconciled released revisions. The v0.4 tag remains bound to the immutable released product state; post-release documentation does not retag or alter the artifact.
