# v0.3 integration ledger

**Worktree:** `/Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/workshop-v03-gemini`  
**Branch:** `josh/v03-gemini-flash-openrouter`  
**Baseline HEAD (before this integration pass):** `4b4e92ab66f86684c2966b01761053ecd9ce8921`  
**Date:** 2026-08-14

Do not publish, notarize, or externally distribute from this folder.

## Baseline

| Item | Value |
| --- | --- |
| Dirty at start | Uncommitted SYN-3227 host-authorization work (`approval.rs`, `approval_policy.rs`, `lib.rs`, renderer modal) |
| Stashes | Unrelated `preserve/aug12-*` stashes; left untouched |
| Desktop version in package/tauri | `0.3.0` |
| About changelog at start | Still `0.2.0` only — **must-fix, addressed** |
| Packaging script at start | Still named `v0.2.0` — **must-fix, addressed** |
| Instance tests at start | Still asserted `v0.2` / `v02` — **must-fix, addressed** |
| Running app | Not captured; no destructive reset |

## Workstream review

| Workstream | Ticket | Source | Review | Landed on this branch? | Notes |
| --- | --- | --- | --- | --- | --- |
| Gemini Flash | SYN-3216 | `7f260b4` | In scope | Yes | OpenRouter target |
| Context | SYN-3220 | `8fc3fae` … `38b2ea3` | In scope | Yes | Settings → Context, cookbook progress, error copy |
| Visual families / Trace V5 / diagrams / splitters | SYN-3217 | `003a36f` | In scope | Yes | `visuals/families/`; flat `templates/` removed |
| Typed approvals | SYN-3227 | `4b4e92a` + `b2651fb` | In scope | Yes | Paid compute, sidecar, credentials, `.synth` `never` ≠ Always Ask |
| VisualsBench / click-to-label | SYN-3218 / SYN-3219 | Earlier on this line | In scope as already-complete | Yes | Not redesigned |
| Reports | — | `agent/v03-reports-complete` `2157f26`…`6171c47` | In scope product-wise | **No** | Stacked on optimizer plugin MCP (`10ec866`). Cherry-pick would pull an out-of-scope plugin. Port later without the plugin. |
| Subagents product surface | SYN-3222 | `2dd1cf0` | In scope | **Yes** | Dedicated rail, child-thread reads, overlap Playwright. Close after packaged CUA. |
| E4 Harbor DEO | SYN-3224 | Workshop `2dd1cf0` + adapter PRs | In scope | **Workshop + adapters yes; matrix no** | Do not close on Craftax Harbor demo. |
| Optimizer plugin MCP | — | `josh/v03-optimizer-plugin-mcp-e2e` | Out of scope | No | Interfaces may exist; plugin is not a v0.3 friends claim |
| E2 / E3 / E5 | SYN-3221 / SYN-3223 | proofs branch | Explicitly deferred | No | |

## Integration changes this pass

- Finished host authorization on `ApprovalBroker::authorize_host`: policy auto-grant, session grants, modal waiter, operator sidecar audit.
- Wired optimizer recipe start (already present) and sidecar install/start/stop/uninstall through the broker.
- Paid-compute UI is a cap-scoped dialog in the transcript pane (not a full-app trap); Escape rejects; Approve focuses.
- Settings → About changelog now leads with `0.3.0`.
- `scripts/release-artifact.sh` and `scripts/test-desktop-instance.sh` identify as `v0.3` / `0.3.0`.
- Added `v0p3_changelog.md`, `docs/launch/v0.3-launch.md`, and copied `docs/launch/v0.3-themes.md`.

## Conflicts

None merged. Reports and E4 were **not** merged, by review: foreign history includes out-of-scope plugin MCP and deferred proof surfaces.

## Release readiness

**Friends ZIP published** from `origin/dev` `c146e83`. See [READY.md](./READY.md) and [PACKAGE.md](./PACKAGE.md).
