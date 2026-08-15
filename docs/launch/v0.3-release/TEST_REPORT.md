# v0.3 test and CUA report

**SHA packaged:** `7f5d90ff73cf96486cb89384678cb35df55a95de` (`josh/v03`).  
**Date:** 2026-08-15

## Passed this pass

| Gate | Result |
| --- | --- |
| `npm run test:visuals` | PASS (113 tests) |
| `cargo check` (desktop lib) | PASS (warnings only) |
| `npm run frontend:build` | PASS |
| `./scripts/release-artifact.sh` stage / record / zip / install | PASS (install target was the release output dir, not `/Applications`) |
| ZIP codesign round-trip | PASS (CDHash `82c5e02626cf9a435ec540686f25400177888f95`) |

## Not run this pass

| Gate | Why |
| --- | --- |
| `npm run test:rust` (full) | Not required to cut the friends ZIP |
| `npm run desktop:ui-gates:bombadil` | Not run on the minted app |
| `npm run desktop:ui-gates:playwright` | Not run on the minted app |
| Installed-app CUA / responsive haze | Artifact exists; CUA was not executed |
| Upgrade-from-v0.2 and fresh-install | Not executed |

## CUA / screenshots

None. Do not treat this folder as an installed-app acceptance record.

## Package

See [PACKAGE.md](./PACKAGE.md).
