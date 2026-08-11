# Synth Desktop provenance — friends v0.1.0 (unnotarized)

Receipt for binding the published friends ZIP to Workshop source and the
backend / Responses-gateway tips observed when this file was written.
No secrets.

**Recorded:** 2026-08-11 (~16:45 UTC)  
**Linear:** [SYN-3183](https://linear.app/synth-ai/issue/SYN-3183/w5-bind-friends-zip-provenance-to-sourceexecbackendgateway-shas)

## Public artifact

| Field | Value |
| --- | --- |
| Product page | https://www.usesynth.ai/download |
| Public ZIP | https://www.usesynth.ai/releases/v0.1.0/Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip |
| GitHub release | https://github.com/synth-laboratories/workshop/releases/tag/v0.1.0 |
| Asset name | `Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip` |
| Size (bytes) | `12879866` |
| SHA-256 | `99c6a45ff9401de42b5ac596e546ad68e867ac20cef1397687163de279ea417f` |
| Signing | ad-hoc (`Signature=adhoc`, TeamIdentifier unset) — **not** Apple-notarized |
| Bundle ID | `com.synth.desktop` |
| CFBundleShortVersionString / CFBundleVersion | `0.1.0` / `0.1.0` |

Verification (2026-08-11): downloaded both the public ZIP and the GitHub
release asset; byte-identical; SHA-256 matched the published digest.
`codesign --verify --deep --strict` on the extracted `.app` succeeded.

## Workshop source that produced the ZIP

| Field | Value |
| --- | --- |
| Tag | `v0.1.0` → `e562f7ee941666fe57f0a68c9ca72fd56e6ab361` |
| Release `targetCommitish` | `e562f7ee941666fe57f0a68c9ca72fd56e6ab361` |
| Qualified RC (identical tree) | `3c39cc61a0d2daa8c51d2e7e4b8d5bc130b3b96f` |
| Tree OID (both commits) | `d458f0fe90883b9de111ef10d424d67707b38cd9` |
| Merge message | Merge pull request #3 (`release/v0.1`) — freeze Synth Desktop v0.1 candidate |

`e562f7ee…` is the merge of the release candidate; its tree matches
`3c39cc61…` exactly. The GitHub release body already names this pair.

### Scope note (post-ZIP main tip)

`origin/main` at receipt time was `0e8af0a` (includes PR #4 /
`a133e70` — Synth Cloud Laguna → dedicated Responses gateway). **Those
commits are not in the friends ZIP.** Friends installs are the tagged
`v0.1.0` tree above unless a replacement artifact is published.

## Inner Mach-O digests (extracted app)

Path relative to `Synth Desktop.app`. All are `Mach-O 64-bit executable arm64`.

| Path | Size | SHA-256 |
| --- | ---: | --- |
| `Contents/MacOS/synth-desktop` | 28053024 | `2545eea0c06d155356a07078a06aee48080dd7d70937e937f69e3df002b31c17` |
| `Contents/MacOS/synth_trace_import` | 6536608 | `1fa0f7a9f5369ef3dbf0282d033ef7f21b44b94284ba824873a13c45dcc709c2` |
| `Contents/MacOS/synth-visuals-mcp` | 760080 | `cae92eeda0a3f2b17a4740b219101210e1a7a06707093277ceb186b5b91ab5cc` |
| `Contents/MacOS/synth-optimizers-mcp` | 758672 | `3e5f914d3ce4247d338429842fff4fbbfcd2649697f620ac8dc242ea743f89b0` |
| `Contents/MacOS/synth-containers-mcp` | 692512 | `cf5dafc42eb45c6a07588cd3227b8cb9586c2c710147b00fb55dd0de11997bff` |

App code directory (adhoc):

| Field | Value |
| --- | --- |
| CandidateCDHashFull (sha256) | `721fd1349b4739b656a5c739457f831b5f83455cc777ee22dcf55ada48f91adf` |
| CDHash (truncated) | `721fd1349b4739b656a5c739457f831b5f83455c` |
| CodeDirectory flags | `adhoc,runtime` |

Bundled `Contents/Resources/services/laguna-daemon` is Python source (no
additional Mach-O helpers under Resources at this build).

## Backend + Responses gateway tips (observed at receipt)

These are the live service identities the friends build is expected to talk
to for account / cloud Laguna (subject to W2 gateway tip decisions and
later promote waves). Re-check `/version` before treating as still current.

| Surface | Endpoint | Observed identity |
| --- | --- | --- |
| Prod backend API | `https://api.usesynth.ai/version` | `git_sha` `3a44f1232874616eacc8c9a6e630d55681eadc57` (`environment=prod`) |
| Staging backend API | `https://api-dev.usesynth.ai/version` | `git_sha` `9b7d26b8b42d5eee08fbb61561e6f1c4971f12ca` (`environment=dev`) — **not** the friends binding tip; recorded for contrast |
| Prod Responses gateway | `https://synth-responses-gateway-prod-production.up.railway.app/version` | short `b69ece9` → full `b69ece94ce560163526cac4268dc2ab83cba9f1b` |
| Staging Responses gateway | `https://synth-responses-gateway-staging-dev.up.railway.app/version` | short `b69ece9` → same full SHA |

Gateway source tip on `synth-responses-gateway` main at receipt
(`59b5b8b20b2a45f4e09560c0e4c5d636fa97ab62`, memory metering) remains
**ahead** of the deployed Railway short SHA — see W2 / H1.

Desktop account/billing default backend for the frozen tree:
`https://api.usesynth.ai`. The tagged friends ZIP predates dedicated gateway
routing and cannot be repaired with a runtime override. Replacement artifacts
must be cut from Workshop main after PR #7, where local, staging, and production
Responses gateway routing is source-owned and unknown profiles fail closed.

## Reproduce checks

```bash
ZIP_URL="https://www.usesynth.ai/releases/v0.1.0/Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip"
curl -fsSL -o /tmp/Synth-Desktop-v0.1.0.zip "$ZIP_URL"
shasum -a 256 /tmp/Synth-Desktop-v0.1.0.zip
# expect 99c6a45ff9401de42b5ac596e546ad68e867ac20cef1397687163de279ea417f

unzip -q /tmp/Synth-Desktop-v0.1.0.zip -d /tmp/synth-desktop-v0.1.0
codesign --verify --deep --strict "/tmp/synth-desktop-v0.1.0/Synth Desktop.app"
shasum -a 256 "/tmp/synth-desktop-v0.1.0/Synth Desktop.app/Contents/MacOS/"*

curl -fsS https://api.usesynth.ai/version
curl -fsS https://synth-responses-gateway-prod-production.up.railway.app/version
```

## Binding summary

```text
ZIP sha256:99c6a45f…417f  (12879866 B)
  ← workshop tag v0.1.0 / e562f7ee…  tree d458f0fe…  (= RC 3c39cc61…)
  ← main Mach-O synth-desktop sha256:2545eea0…1c17
  ↔ prod backend 3a44f123…dc57
  ↔ prod+staging gateway b69ece94…9f1b (short b69ece9)
```
