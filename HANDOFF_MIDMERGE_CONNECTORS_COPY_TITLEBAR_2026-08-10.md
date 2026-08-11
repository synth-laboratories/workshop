# HANDOFF — mid-merge of codex/context-compaction into dev (2026-08-10 ~10:45)

## Where you are RIGHT NOW
`~/Documents/GitHub/workshop`, branch `dev` at `75d4472`, **in an unfinished merge of
`codex/context-compaction`** (`.git/MERGE_MSG` exists). All conflicts are resolved and
staged; TypeScript is clean; node tests are 83/84 (the 1 fail is pre-existing, see below).
**Not yet done: targeted Playwright run → `git commit` (concludes the merge) → push →
`npm run desktop:install`.** A concurrent agent is ALSO live-editing this tree in bursts
(Muse Glimmer lane, plus someone just modified ChatTranscript/sessionView/landing.ts —
the ` M` unstaged entries). Check mtimes before committing; do not commit their unstaged
work blindly.

## What this merge brings (branch `codex/context-compaction`, worktree
`/Users/joshuapurtell/Documents/Codex/2026-08-10/im/work/workshop-compaction`)
- `7d90eaa` XS manual compaction reliability (renderer only, no Rust)
- `bc6801a` titlebar trim
- `556926d` Connectors nav removal + message copy buttons (I committed the worktree's
  uncommitted diff as this commit; the worktree is now clean)
- `HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md` (625-line Visuals handoff) rides along.

## Conflict resolutions already made (staged)
- **App.tsx**: kept the titlebar trim BUT restored `runtime-status` pill and
  `toggle-inference-rail` + their definitions (`localRuntimePresentation`, `IconPulse`,
  `setInferenceRailOpen`). Reason: the branch's trim deleted `runtime-status`, which
  `tests/playwright/browser.fixture.ts:80` waits on — every one of the 14 Playwright
  suites boots through it — and its own specs still referenced it (the branch was never
  green). Removed for real: avatar "S" button (`open-account-settings` dup testid) and
  the titlebar Models button.
- **ChatTranscript.tsx**: branch's `local-user-message` wrapper + `CopyMessageButton`
  merged WITH dev's image-attachments block and `{body ? <p/> : null}` guard.
  NOTE: after my resolution someone modified this file again externally (intentional,
  keep their version; it still has copy buttons + compaction glyph work).
- **Specs aligned to the real trim** (staged): `layout-invariants.spec.ts` titlebar test
  now expects pill visible + account/models buttons count 0; `design-debt.spec.ts`
  "titlebar Models opens…" test removed; `design_debt.test.mjs` titlebar test rewritten
  (asserts avatar/models gone, `setView settings/account` still wired via sidebar).

## Remaining steps (in order)
1. `npx tsc --noEmit -p apps/synth_desktop/tsconfig.json` (was clean; re-verify after the
   external ChatTranscript edit).
2. `npm run build --workspace @synth/synth-desktop && npx playwright test --config
   apps/synth_desktop/playwright.config.ts layout-invariants design-debt
   sidebar-navigation get-started` — these cover the trim/connectors/copy surface.
3. `git commit` (merge commit message already drafted in `.git/MERGE_MSG`), decide
   whether to fold in the concurrent lane's ` M` unstaged files (only if their lane is
   quiet ≥3 min and `cargo check` + tsc pass).
4. `git push origin dev`; `npm run desktop:install` (takes ~11s now).

## Known reds — do NOT chase as regressions
- `a11y_surface.test.mjs` "v0.2 Intern bridge remains typed…": concurrent lane added
  `assert.ok(!app.includes("nativeIntern.createSession"))` before removing the call.
  Theirs to finish.
- Playwright pre-existing at 73dbb6f/75d4472: optimizer-banking77 ×3, poolside-polish
  steer/enqueue, runtime-regressions provider-switch mid-chat.

## Today's context (already pushed, `f188204..75d4472`)
- All 4 stashes landed then dropped (globe-icon removal, model-performance tracking,
  perf-tracker-v2, docs); recovery commits `81f4088`/`28f9120`.
- Build speed: `crate-type=["rlib"]` + `[profile.release] incremental=true` →
  one-line-change `desktop:install` = **11s** (first build after any profile change pays
  ~90s once). MCP release-bin build removed from install (installed app resolves adapters
  via `target/debug` fallback; sidecar bundling is launch-program work).
- Full narrative + gotchas: memory files `workshop-stash-landing-and-build-speed`,
  `workshop-local-gate-gotchas`, `workshop-v01-release-campaign`; polish.md 2026-08-10
  entries.
