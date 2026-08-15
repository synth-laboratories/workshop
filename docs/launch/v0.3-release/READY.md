# v0.3 ready?

**No.** Workshop `0.3.0` is not release-ready.

## Why not

1. Changelog, identity, launch docs, and focused-test updates are still uncommitted. Packaging (`scripts/release-artifact.sh`) refuses a dirty tree.
2. Reports is not on this branch. It lives on `agent/v03-reports-complete`, stacked on optimizer plugin MCP, which is out of v0.3 scope.
3. SYN-3222 (Codex-like subagent rail, child workspace, overlapping spawn/wait) is only a grouped visual.
4. SYN-3224 E4 Harbor DEO has no canonical run-ID / config / raw-evidence package on this branch.
5. Installed-app CUA, Bombadil, Playwright UI gates, and a clean release build were not run on a committed SHA.
6. Linear could not be closed from this environment (Linear MCP needs Cursor desktop auth).

## What is true

- Desktop package/tauri/instance launcher identify as `0.3.0` / `v0.3` (uncommitted script/About edits still need a commit).
- Settings → About changelog leads with 0.3.0 (uncommitted).
- SYN-3216, SYN-3220, SYN-3217, and SYN-3227 (including host authorization) are committed on `josh/v03-gemini-flash-openrouter` at `b2651fb`.
- Focused tests that ran this pass passed (see [TEST_REPORT.md](./TEST_REPORT.md)).

## Next actions to get to yes

1. Commit changelog, identity, launch docs, and focused-test updates so the tree is packageable.
2. Port Reports without the optimizer plugin MCP history, or explicitly drop Reports from the friends claim.
3. Either finish SYN-3222 or drop it from the v0.3 claim.
4. Attach E4 evidence or drop the E4 friends claim.
5. Run `npm run desktop:verify:fast`, Bombadil, Playwright, then `scripts/release-artifact.sh` on that exact SHA.
6. Install the artifact, run CUA, record checksums, close Linear.
