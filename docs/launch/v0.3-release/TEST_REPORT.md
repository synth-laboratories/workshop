# v0.3 test and CUA report

**SHA tested:** working tree on top of `b2651fb` (changelog/identity/docs still dirty).  
**Date:** 2026-08-14

## Passed this pass

| Gate | Result |
| --- | --- |
| `npm run typecheck` | PASS |
| `npm run test:visuals` | PASS (108 tests) |
| Approval broker + policy unit tests (`cargo test --lib approval`) | PASS (15) |
| Paid-compute cap receipt test | PASS |
| Focused node tests (invariants, activity, design debt, oauth leak) | PASS (32) |
| `./scripts/test-desktop-instance.sh` | PASS (`v0.3` / `0.3.0` / `v03`) |

## Not run this pass

| Gate | Why |
| --- | --- |
| `npm run test:rust` (full) | Only approval- and cap-focused lib tests were run |
| `npm run desktop:verify:fast` | Left for the clean-SHA package cut |
| `npm run desktop:ui-gates:bombadil` | Requires a built app |
| `npm run desktop:ui-gates:playwright` | Requires a built app |
| Installed-app CUA / responsive haze | No packaged artifact yet |
| Upgrade-from-v0.2 and fresh-install | No packaged artifact yet |

## CUA / screenshots

None. Do not treat this folder as an installed-app acceptance record.

## Package

None. Canonical command after a clean commit:

```bash
./scripts/release-artifact.sh all "$SYNTH_RELEASE_ROOT"
```

Default ZIP name is now `Synth-Desktop-v0.3.0-macOS-arm64-UNNOTARIZED.zip`.
