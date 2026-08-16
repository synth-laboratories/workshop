# v0.4 package

- Product: Synth Desktop
- Version: 0.4.0
- Channel: friends
- Architecture: macOS arm64
- Signing: ad-hoc
- Notarization: none
- Artifact: `Synth-Desktop-v0.4.0-macOS-arm64-UNNOTARIZED.zip`
- ZIP SHA-256: `a1f2e882ccc7ac4eeab31ce55b1548a11114cd6b3c10f5290a4e94cecaa114ec`
- ZIP bytes: `19360702`
- App CDHash: `991e1029fe78179f2b18be54f603fff2ac25bd54`
- Workshop source: `9fffe8c8b5ede969b734118c04935fe42cc6baf1`
- Containers source: `2826be633a3d86b028e2c8ebb0e9d587d8b794cf`

The release script built the clean committed source, copied the release adapters, signed the staged app, verified the signature, ZIP-round-tripped the bundle, compared CDHashes, and installed an isolated copy. That installed copy subsequently passed launch, optimizer-update, paid Banking77, optimizer-visual, and advanced-trace CUA acceptance.
