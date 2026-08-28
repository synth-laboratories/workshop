# Synth Desktop provenance — friends v0.1.0 path (unnotarized)

Receipt for binding the published friends ZIP to Workshop source.
No secrets.

**Observed (download check):** 2026-08-12 (~14:13 UTC)
**Prior intended receipt:** 2026-08-11 (~19:10 UTC) — SHA `660a8e…` was **not** what the public path served at CUA time
**Linear:** [SYN-3183](https://linear.app/synth-ai/issue/SYN-3183/w5-bind-friends-zip-provenance-to-sourceexecbackendgateway-shas)

## Public artifact (currently served)

Verified by downloading
`https://www.usesynth.ai/releases/v0.1.0/Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip`
on 2026-08-12 (CUA receipt + re-check).

| Field | Value |
| --- | --- |
| Product page | https://www.usesynth.ai/download |
| Public ZIP | https://www.usesynth.ai/releases/v0.1.0/Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip |
| Public path / URL contract | `v0.1.0` (filename unchanged) |
| Asset name | `Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip` |
| Size (bytes) | `13091422` |
| SHA-256 | `d317760fe414798c9c29ce3bb0db599beed25489f6a35f53650ac4d4ecac01a5` |
| Signing | ad-hoc (`Signature=adhoc`, TeamIdentifier unset) — **not** Apple-notarized |
| Bundle ID | `com.synth.desktop` |
| CFBundleShortVersionString / CFBundleVersion | `0.1.0` / `0.1.0` |
| Main binary SHA-256 | `304401b530b269c733cfdbfb4383f0bb1d0ca045429c4552ef49af744a4bb953` |
| CandidateCDHashFull (sha256) | `b5dd32e77b144929b10b883e88d87662bade255d00575658e7a86e6a3cb759a4` |

**Workshop source SHA for these bytes:** unknown / not bound in this repo at observation time.
Re-bind source + FE `SYNTH_DESKTOP_STABLE_ARTIFACT_SHA256` on the next friends cut; do not treat the superseded `660a8e…` row as the live gate.

### Superseded digests at the same public path

| SHA-256 | Notes |
| --- | --- |
| `660a8e2b2f7985da54a66355b70437c7ec123c120d9978d017483ba248a5571b` | 2026-08-11 intended friends rebuild from workshop `077579a` (CFBundle `0.2.0`); **not** what the URL served as of 2026-08-12 |
| `99c6a45ff9401de42b5ac596e546ad68e867ac20cef1397687163de279ea417f` | Earlier tagged `v0.1.0` friends ZIP |

## Inner Mach-O digests (extracted from currently served ZIP)

Path relative to `Synth Desktop.app`. All are `Mach-O 64-bit executable arm64`.

| Path | Size | SHA-256 |
| --- | ---: | --- |
| `Contents/MacOS/synth-desktop` | 28074144 | `304401b530b269c733cfdbfb4383f0bb1d0ca045429c4552ef49af744a4bb953` |
| `Contents/MacOS/synth_trace_import` | 6536608 | `dfbf791c733a5cdd81e818aa78298975cb894b47460f4c841aab2267a7afa07b` |
| `Contents/MacOS/synth-visuals-mcp` | 760080 | `e476c1fa5d28d15072a842dd8dd8f9f937696e209d903108ae83246619413bbd` |
| `Contents/MacOS/synth-optimizers-mcp` | 758672 | `182cdcd6ef33841b6101e00d173a74fc5829d14fdc5c607bb827b29b199b85b3` |
| `Contents/MacOS/synth-containers-mcp` | 692512 | `89289087572c984fe04e4b79e8b3e6caa39bdcf6bd07c58169354cb2cf8de853` |

## Backend + Responses gateway tips

Re-check `/version` before treating as still current.

| Surface | Endpoint | Note |
| --- | --- | --- |
| Prod backend API | `https://api.usesynth.ai/version` | Re-check `git_sha` at promote time |
| Staging backend API | `https://api-dev.usesynth.ai/version` | Contrast only |

Desktop account/billing default backend remains `https://api.usesynth.ai`.

## Reproduce checks

```bash
ZIP_URL="https://www.usesynth.ai/releases/v0.1.0/Synth-Desktop-v0.1.0-macOS-arm64-UNNOTARIZED.zip"
curl -fsSL -o /tmp/Synth-Desktop-v0.1.0.zip "$ZIP_URL"
shasum -a 256 /tmp/Synth-Desktop-v0.1.0.zip
# expect d317760fe414798c9c29ce3bb0db599beed25489f6a35f53650ac4d4ecac01a5

# After extract:
# CFBundleShortVersionString / CFBundleVersion == 0.1.0
# codesign --verify --deep --strict "Synth Desktop.app"
```

## Frontend / Vercel binding

- Frontend publishes the ZIP bytes under the same `public/releases/v0.1.0/…` path.
- Production `SYNTH_DESKTOP_STABLE_ARTIFACT_SHA256` must match the **currently served** digest
  (`d317760fe414798c9c29ce3bb0db599beed25489f6a35f53650ac4d4ecac01a5`) until the next overwrite.
- Keep `SYNTH_DESKTOP_STABLE_ARTIFACT_URL` on the same `/releases/v0.1.0/…UNNOTARIZED.zip` HTTPS path.
- When cutting a new friends ZIP, update this file **and** the Vercel env in the same change set.
