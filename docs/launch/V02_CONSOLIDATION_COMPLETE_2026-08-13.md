# v0.2 code consolidation — 2026-08-13

This note supersedes the repository and dirty-tree inventory in
`V0.2_REMAINING_WORK_2026-08-13.md`. It records the code-consolidation phase;
release acceptance and promotion remain separate.

## Dev integration tips

| Repository | Integration tip before this note | Result |
| --- | --- | --- |
| Containers | `1b8ec1191d73` | Harbor, healthbench, traces, heartbeat credential rotation, and regression coverage consolidated |
| Optimizers | `bc7b5a5b3c89` | GEPA and tunnel work consolidated; reconciled Containers dependency locked |
| Optimizers Beta | `b13ec3b47082` | OHCO, hosted SFT, sampling replay, and Workshop config-contract route consolidated |
| Backend | `1b3a469e7c3c` | Main/dev plus the six live catalog, Responses, network, runtime, Buck2, and docs lanes consolidated |
| Frontend | `b06b2f1d3a00` | PR 247, main release state, dependency lock, and published Craftax asset resolution consolidated |
| Workshop | `6126c73365c9` | Main/dev, visuals, modern stack, account/API-key, OAuth, SFT contract, and visual layout fixes consolidated |

All six integration worktrees were clean at the tips above. The integration
branch in each repository is `agent/v02-dev-consolidation-20260813`; the target
trunk is `dev`.

## Checks completed during consolidation

- Containers: full suite, `311 passed, 8 skipped`.
- Optimizers: Rust workspace excluding the macOS PyO3 extension-link target
  passed (`synth_gepa` 24 and platform 27); the tunnel integration test passed.
  The unexcluded workspace invocation reached only the environment-specific
  PyO3 linker failure.
- Optimizers Beta: `cargo check --workspace` and the full Rust workspace passed
  (Beta 10, GoEx 51, SFT 50).
- Backend: 64 focused tests passed after aligning the allowance assertions with
  the current plan catalog.
- Frontend: content build, TypeScript check, and 14 desktop-release tests passed.
- Workshop: focused hosted-SFT, OAuth, and settings Rust tests passed; protocol
  regeneration was zero-diff; production Vite build passed. Bombadil exercised
  960, 1172, and 1280 px widths with no violations after proving both intrinsic
  card height and responsive stacked/split-pane containment.

## Visual proof carried forward

The rebuilt Workshop proof remains visual `vis_d54916fb238942ffaf79992487da8513`,
revision 3: all 10 rollout lanes finished; all 284 envelopes replayed (274
snapshots and 10 terminals); the selected lane showed 28 semantic and 28 raw
events, reward 4.90, and five achievements; the terminal state remained
`stream ready`. No new rollouts or paid evaluations were launched.

`docs/visuals_issues.md` continues to own the clipped provenance line,
redundant terminal status, zoom/width acceptance, and the proposed single
terminal receipt.

## Preserved, deliberately not landed

- Workshop's seven captured stash states remain reachable as
  `preserve/aug12-stash-0` through `preserve/aug12-stash-6`. Their v0.2 content
  was either landed or superseded. The v0.3-only visual-library primary-list
  resize remains in `preserve/aug12-stash-4`.
- The live Containers Harbor worktree and the older Containers trace worktree
  were not rewritten or deleted. Their relevant committed work is represented
  in the integration tip; any still-live process or uncommitted experiment must
  be retired by its owner.
- Generated Backend Muse proof and Optimizers MAPO output were excluded from
  source commits.
- Frontend's large dev/main history difference was not treated as a release
  promotion. Only main's release-publication state was back-merged into dev.

## Remaining release phase

Code consolidation does not itself ship v0.2. The short acceptance window must
still submit Workshop's generated Craftax SFT TOML to Optimizers Beta built from
these reconciled tips, triage the four release gates, promote the intended
release branches, build/sign the final artifact, and publish its receipt.
