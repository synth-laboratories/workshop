# v0.4 package

- Product: Synth Desktop
- Version: 0.4.0
- Channel: friends
- Architecture: macOS arm64
- Signing: ad-hoc
- Notarization: none
- Artifact: `Synth-Desktop-v0.4.0-macOS-arm64-UNNOTARIZED.zip`
- ZIP SHA-256: `29eb5b4dba4e2cf7bb6014f750914e015cf3accc954d8b41ca82754530f98b1b`
- ZIP bytes: `19360540`
- App CDHash: `62eb8c57112f23b9b32d17dc6d312b9f91d95e8d`
- Workshop source: `bf09eb10b1bbd8449c079a0cf0657bf23a2ebe9d`
- Containers source: `2826be633a3d86b028e2c8ebb0e9d587d8b794cf`

The release script built the clean committed source, copied the release adapters, signed the staged app, verified the signature, ZIP-round-tripped the bundle, compared CDHashes, and installed an isolated copy without launching it.
