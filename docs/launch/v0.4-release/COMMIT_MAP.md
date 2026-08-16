# v0.4 commit map

| Surface | Revision | Purpose |
| --- | --- | --- |
| Workshop product bytes | `9fffe8c8b5ede969b734118c04935fe42cc6baf1` | Consolidated replay/capture, TPS, elapsed-time, transcript trace viewer, fixes, Rust formatting, and optimizer v0.2.14 pin |
| Public cookbook | `9d82a6a8785a4bcf2a214ee8672f076063bb0f98` | Reviewed Banking77 GEPA v2 producer contract and contract tests |
| Synth optimizers | `4746e085be5035d9c804074e4342168903359335` / `v0.2.14` | Database migration and installed-version heartbeat fixes |
| Containers | `2826be633a3d86b028e2c8ebb0e9d587d8b794cf` | Clean dedicated checkout used by the release builder |
| Public website catalog | `4c7c5d5df7e057e3b0e5da4aef5221e482c20fdc` | v0.4 release catalog, notes, artifact URL, and exact final SHA-256 |

Promotion policy is `v0.4/* -> dev -> main/tag`. The v0.5 implementation branch must be created from the reconciled released revision, not directly from an unmerged release candidate.
