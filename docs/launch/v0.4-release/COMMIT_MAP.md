# v0.4 commit map

| Surface | Revision | Purpose |
| --- | --- | --- |
| Workshop product bytes | `9fffe8c8b5ede969b734118c04935fe42cc6baf1` | Consolidated replay/capture, TPS, elapsed-time, transcript trace viewer, fixes, Rust formatting, and optimizer v0.2.14 pin |
| Public cookbook | `9d82a6a8785a4bcf2a214ee8672f076063bb0f98` | Reviewed Banking77 GEPA v2 producer contract and contract tests |
| Synth optimizers | `4746e085be5035d9c804074e4342168903359335` / `v0.2.14` | Database migration and installed-version heartbeat fixes |
| Containers | `2826be633a3d86b028e2c8ebb0e9d587d8b794cf` | Clean dedicated checkout used by the release builder |
| Public website catalog | `e78151dc90b72d8d2b23cffb56e5fe184f7829f7` | Final v0.4 release catalog, notes, immutable artifact, and exact SHA-256 |
| Public website `main` | `06943bd51d17531444ce44a083783da8fc94aa66` | Production promotion of the v0.4 release catalog |
| Workshop `dev` promotion | `b6c49b0a9be88476b00490cb505f4ba0a8a37dd8` | Reviewed v0.4 release-branch merge |
| Workshop `main` / tag | `e12276d05d4513ad9e74d93c2c54bbc616e92926` / `v0.4.0` | Released trunk revision and immutable release tag |

Promotion policy was `v0.4/* -> dev -> main/tag`. The v0.5 implementation branches were created from the reconciled released revisions; Workshop's setup receipt is `0d3d292`.
