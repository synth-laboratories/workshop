# Synth Desktop provenance — friends v0.1.0 path (unnotarized)

Receipt for binding the published friends ZIP to Workshop source and the
backend / Responses-gateway tips observed when this file was written.
No secrets.

> **Canonical release process (v0.2.0 and later):** run
> [`scripts/release_desktop.sh`](../../scripts/release_desktop.sh) from a clean
> checkout — e.g. `./scripts/release_desktop.sh 0.2.0`. It builds from a clean
> detached worktree of `origin/main`, signs (ad-hoc + hardened runtime) and
> strict-verifies in place, packages the `…-UNNOTARIZED.zip` with `ditto`, and
> emits `dist/release/v<version>/RECEIPT.txt` plus a paste-ready section for
> this file and the frontend `desktopRelease` constants. One script run = one
> digest; do **not** hand-run the steps below for a public artifact. The
> sections below document the v0.1.0-era manual path the script encodes.

**Recorded:** 2026-08-11 (~19:10 UTC)  
**Linear:** [SYN-3183](https://linear.app/synth-ai/issue/SYN-3183/w5-bind-friends-zip-provenance-to-sourceexecbackendgateway-shas)

## Public artifact

| Field | Value |
| --- | --- |
| Product page | https://www.usesynth.ai/download |
| Public ZIP | https://www.usesynth.ai/releases/v0.1.0/Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip |
| Public path / URL contract | `v0.1.0` (filename unchanged) |
| Asset name | `Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip` |
| Size (bytes) | `12870615` |
| SHA-256 | `660a8e2b2f7985da54a66355b70437c7ec123c120d9978d017483ba248a5571b` |
| Previous SHA-256 (superseded) | `99c6a45ff9401de42b5ac596e546ad68e867ac20cef1397687163de279ea417f` |
| Signing | ad-hoc (`Signature=adhoc`, TeamIdentifier unset) — **not** Apple-notarized |
| Bundle ID | `com.synth.desktop` |
| CFBundleShortVersionString / CFBundleVersion | `0.2.0` / `0.2.0` |

This rebuild **overwrites** the friends download at the stable `v0.1.0` public
path so existing download links keep working. Inner bundle version is `0.2.0`
(routing / gateway work from workshop PRs #8 / #9); do not treat CFBundle as
the public path version.

Verification: local ad-hoc friends build receipt
`workshop-077579a-friends-adhoc-20260811T190307Z`;
`codesign --verify --deep --strict` on the staged `.app` succeeded
(`codesign-verify.txt`).

## Workshop source that produced the ZIP

| Field | Value |
| --- | --- |
| Source SHA | `077579ab8be0852ee7958af707e1b51e50989d52` |
| Includes | Workshop PRs **#8 / #9** Responses gateway routing (local / staging / production; unknown profiles fail closed) |
| Build kind | Friends ad-hoc unnotarized macOS arm64 ZIP |

### Supersedes

Earlier friends ZIP at the same public path was cut from tagged `v0.1.0`
(`e562f7ee941666fe57f0a68c9ca72fd56e6ab361` / tree `d458f0fe…`) and did **not**
include dedicated gateway routing. That digest
`99c6a45f…` is obsolete once the frontend overwrite + Vercel
`SYNTH_DESKTOP_STABLE_ARTIFACT_SHA256` update land.

## Inner Mach-O digests (extracted app)

Path relative to `Synth Desktop.app`. All are `Mach-O 64-bit executable arm64`.
Digests from receipt `macho.sha256`; sizes from staged binaries.

| Path | Size | SHA-256 |
| --- | ---: | --- |
| `Contents/MacOS/synth-desktop` | 28038016 | `5b3d7feee34a7bb3ca76dde4fc734299c001a760bffb961903c06b79a1280a7f` |
| `Contents/MacOS/synth_trace_import` | 6529904 | `a22ebfee5f769ff28def4912ae1193f646168469c258f6d60fe0166f0e2b8184` |
| `Contents/MacOS/synth-visuals-mcp` | 759792 | `50103b4967649b0a439382d3aebad9816ceb762dc4757eda424d5ce06a0f8052` |
| `Contents/MacOS/synth-optimizers-mcp` | 758368 | `463b2b9693b211e550954ea2bd375dc5e2a11744bb6ca195d00579384aba51f0` |
| `Contents/MacOS/synth-containers-mcp` | 692224 | `59fe8aacae46fd760847b0a66ed65a9ff49444966b893b29b7a5dab824084ac2` |

App code directory (adhoc), from `codesign-details.txt`:

| Field | Value |
| --- | --- |
| CandidateCDHashFull (sha256) | `35d73ff1f3529c2c4b39f16377d8818a375ecbca0e5fe511cb49c1cdaf237abb` |
| CDHash (truncated) | `35d73ff1f3529c2c4b39f16377d8818a375ecbca` |
| CodeDirectory flags | `adhoc,runtime` |

## Backend + Responses gateway tips (observed at receipt)

Re-check `/version` before treating as still current. This friends rebuild is
expected to use source-owned gateway routing from `077579a` rather than the
pre-#8/9 tree.

| Surface | Endpoint | Note |
| --- | --- | --- |
| Prod backend API | `https://api.usesynth.ai/version` | Re-check `git_sha` at promote time |
| Staging backend API | `https://api-dev.usesynth.ai/version` | Contrast only |
| Prod Responses gateway | Railway `/version` for production gateway | Must match profile routing in this build |
| Staging Responses gateway | Railway `/version` for staging gateway | Must match profile routing in this build |

Desktop account/billing default backend remains `https://api.usesynth.ai`.

## Reproduce checks

```bash
ZIP_URL="https://www.usesynth.ai/releases/v0.1.0/Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip"
curl -fsSL -o /tmp/Synth-Desktop-v0.1.0.zip "$ZIP_URL"
shasum -a 256 /tmp/Synth-Desktop-v0.1.0.zip
# expect 660a8e2b2f7985da54a66355b70437c7ec123c120d9978d017483ba248a5571b

# After extract:
# CFBundleShortVersionString / CFBundleVersion == 0.2.0
# codesign --verify --deep --strict "Synth Desktop.app"
```

## Frontend / Vercel binding

- Frontend PR publishes the ZIP bytes under the same `public/releases/v0.1.0/…` path.
- Production must set `SYNTH_DESKTOP_STABLE_ARTIFACT_SHA256` to
  `660a8e2b2f7985da54a66355b70437c7ec123c120d9978d017483ba248a5571b` on Vercel
  projects `frontend` and `synth-frontend` (team `synth-ff365c23`) **after** the
  ZIP deploy lands. Keep `SYNTH_DESKTOP_STABLE_ARTIFACT_URL` on the same
  `/releases/v0.1.0/…UNNOTARIZED.zip` HTTPS path.
