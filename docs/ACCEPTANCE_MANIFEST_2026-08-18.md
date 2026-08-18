# v0.5 release acceptance manifest

This is the consolidated source manifest for the v0.5 friends-preview release.
The distributable is an explicitly unnotarized macOS arm64 preview; it is not a
stable-catalog or notarized production release.

## Frozen source pins

| Component | Branch | Commit / version | Verification |
|---|---|---|---|
| Workshop | `v0.5/implementation` | `bc7527fda074e1364681bc460d341e7fc693ddf9` | pushed; clean `desktop:verify` green |
| Containers | `v0.5/integration` | `e1b9cb13cd86bce43ff6c41b3f0b0ecbed3ab3e2` / `0.4.1.dev20260817` | pushed; 364 passed, 8 skipped |
| Optimizers | `v0.5/integration` | `2ed30aa725ae8bc4c7da959c253dc6c1edd7374a` / `0.2.14` | pushed; 51 passed |
| Public cookbooks | `v0.5/implementation` | `318b0ed9944c3388f8830fd912013d442d8ae33d` | pushed; producer integrity 3 passed |
| Frontend | `v0.5/implementation` | `518addde8707cb25522f8e8fdf8b984ffe522ec6` | pushed; typecheck and 41 focused release tests passed |
| Docs | `v0.5/integration` | `0654c93aa8a0089f8b961e83c66b3c3e1e3c480c` | pushed |

The Workshop SHA above is the verified code head. The documentation commit that
records this manifest will be the final `v0.5/implementation` head before the
single merge to `main`; it must not change executable source.

## Frozen Workshop gate

`npm run desktop:verify` passed from the clean Workshop code head:

- TypeScript typecheck: passed.
- Rust unit and integration tests: passed; the real external Trace bundle test
  and one doc test remain explicitly ignored.
- Desktop instance contract: passed.
- Playwright: 235 passed, 2 intentionally skipped.

Cross-repository gates passed independently:

- Containers: 364 passed, 8 skipped.
- Optimizers: 51 passed.
- Cookbook GEPA v2 producer integrity: 3 passed.
- Frontend: typecheck, release catalog verifier, and 41 focused tests passed.

## Workflow acceptance represented in v0.5

- Banking77 evaluation: fixed-cardinality, retained synchronous rewards and
  evidence, chat-owned terminal experiment visual.
- Banking77 GEPA: ten requested candidates, paired gate evidence, distinct
  proposal accounting, separated selection and deployment verdict.
- HealthBench evaluation: fail-closed policy and grader credential preflight,
  separate usage lanes, no rollout dispatch when either credential is absent.
- Craftax evaluation and GEPA: exact-recipe admission, no silent recipe-family
  substitution, retained traces and distribution-oriented visual contracts.
- Hosted SFT: typed progress/usage/result projection and durable visual replay.

These workflows are test and dogfood fixtures, not a requirement to publish a
Craftax OCI image or distribute a benchmark environment inside the core
Workshop release. v0.6 will remove container-specific product logic from core
Workshop and keep benchmark recipes in testing/evaluation packages.

## Friends-preview packaging contract

- Build only from a new clean detached worktree at the final merged `main` SHA.
- Supply clean detached Containers and cookbook roots at the pins above.
- Stage and record before creating the ZIP.
- Expected filename:
  `Synth-Workshop-v0.5.0-macOS-arm64-FRIENDS-PREVIEW-UNNOTARIZED.zip`.
- Publish as a GitHub prerelease with an unambiguous unnotarized warning,
  SHA-256 checksum, and provenance JSON.
- Do not flip the public stable catalog, describe the artifact as notarized, or
  use the production `v0.5.0` tag for this preview.
- Repeatable automated acceptance uses the first-party QA driver. External CUA
  remains a separately provisioned manual smoke lane because macOS may prompt
  for ChatGPT/Codex Screen Recording permission.

The actual merged Workshop SHA, app path, bundle identity, ZIP checksum,
prerelease tag, and download URL are appended to the release provenance after
the clean build completes.
