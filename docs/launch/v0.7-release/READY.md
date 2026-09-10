# v0.7 readiness

Status: **v0.7.4 shipped** (ad-hoc signed, unnotarized) on 2026-08-22.

This file previously gated an unreleased `v0.7.0`. That gate is **superseded**: v0.7.0 was never
published, and the v0.7 line shipped as a sequence of patch releases under explicit release-owner
waivers. The shipped facts live in `POST_RELEASE.md`; the measured suite counts live in
`TEST_REPORT.md`.

## What is true for v0.7.4

- Workshop `937a316f` (tree `8df5a3e5`), Containers `e1df8c6`, `synth-mlx-rl` `5d6db143`.
  All three worktrees were clean at freeze; the packaged app embeds the pinned MLX wheelhouse.
- Clean-tree suites green on the frozen commit: Rust 1,305 passed / 0 failed / 8 ignored;
  Playwright 251 passed / 0 failed / 2 skipped; Containers 489 passed / 10 skipped;
  TypeScript typecheck clean; run-progress node suites 36 passed.
- Artifact built, ad-hoc signed, and round-trip verified from both ZIP and DMG at a stable CDHash.

## What is explicitly NOT claimed

- No Developer ID signature and no Apple notarization (no non-Keychain mechanism exists; see
  `POST_RELEASE.md`).
- No GitHub Release asset mirror.
- No fresh workload training E2E: ALFWorld, Craftax, HealthBench, hosted CISPO, Harvey/OpenRouter,
  and the full Banking77 training replays were deferred and are not represented as passing.
- No packaged Computer Use / lifecycle acceptance for this candidate.

Hosted CISPO is fail-closed and stays that way until an authenticated `cispo.slime.v1` canary is
durably admitted.
