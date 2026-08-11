# Workshop v0.1 — frozen product contract and owner matrix

**Gate:** friends-release (Gate F) then public-launch (Gate P)  
**Frozen:** 2026-08-10  
**Release driver / no-go authority:** Joshua Purtell (pending named deputies below)  
**This file is the candidate contract.** Claims that are not listed here are out of scope for v0.1.

## Supported surfaces (in)

- Workshop agent (Codex app-server child, approval/sandbox as shipped)
- Local Laguna XS inference (managed download, memory controls, first response)
- Synth Cloud + OpenRouter cloud targets (credential-brokered; child never sees long-lived keys)
- Containers inventory + Craftax Rust GameBench
- Shared Visual / Trace V5 inspector
- GEPA visualization (honestly labeled; not a launch claim of optimizer quality)
- Usage sheet + Account snapshot (plan, allowance, windows)
- Settings (authorized providers, tariffs from the native catalog, account, Laguna, Whisper)

## Explicitly out (Intern absent)

Intern is **not** in picker, navigation, search, docs, screenshots, videos, or marketing. Re-entry criteria remain v0.2 work. GELO/SFT may appear only as honestly labeled `[alpha]` safe states; they are not Gate F promises.

## Hardware / OS / models

| Constraint | v0.1 contract |
|---|---|
| Hardware | Apple Silicon Mac only |
| OS | macOS 14+ (Sonoma or newer) |
| Memory | 16 GB minimum; 32 GB recommended for local Laguna XS |
| Disk | enough free space for the `.app` plus the local Laguna model revision shipped with the candidate |
| Cloud models | Synth Cloud Laguna S 2.1 (plan-metered); OpenRouter GPT 5.6 Luna + Laguna S 2.1 when the user supplies a key |
| Local model | Laguna XS 2.1 (revision pinned on the signed artifact) |
| Billing tiers | Free $0 / Starter $20 / Pro $200 monthly; allowances 0 / 2000 / 20000 cents from Autumn entitlements |

## Owner matrix

Fill named humans before Gate F. Role slots must not stay empty.

| Role | Owner | Deputy |
|---|---|---|
| Release driver / no-go | Joshua Purtell | TBD |
| Desktop | TBD | TBD |
| Web / download / content | TBD | TBD |
| Auth (Clerk / device-init / pairing) | TBD | TBD |
| Backend / billing / metering | TBD | TBD |
| Laguna | TBD | TBD |
| Evals / Trace V5 / CRAFTAX-LUNA-010 | TBD | TBD |
| CUA tester (artifact-bound) | TBD (must not be the implementer) | TBD |
| Independent reviewer | TBD (must not be the implementer) | TBD |
| Incident channel | TBD | TBD |
| Rollback owner | TBD | TBD |

## Candidate revisions (working tree, 2026-08-10)

These are **not** Gate F receipts. They are the integration baseline this freeze started from. Gate F/P require clean, remote-reachable SHAs after review/push, plus one signed artifact.

| Component | Branch / worktree | SHA / note |
|---|---|---|
| Workshop Desktop | `workshop-v0.1` `release/v0.1` | local tip ahead of origin (settlement + tariff catalog + Trace V5 driver landed). Review before push. |
| Evals launch harness | `evals-workshop-v01` `release/workshop-v0.1-evals` | base `fd02ce7e5` + CRAFTAX-LUNA-010 harness in worktree (20/20 unit + typecheck). Do not use intern-dirty `evals/` tip. |
| Backend account + billing | `backend-desktop-account-snapshot` `feat/desktop-account-snapshot` | `ac9ae580f` + Autumn checkout adapter + fake-Autumn + `smr_spend` on Starter/Pro (14 units green). |
| Backend metering | gateway settlement on same snapshot line | default-on for `local`/`staging`/`dev`; prefer provider `usage.cost`. Do not rebuild a second settlement path. |
| Frontend upgrade deep link | `frontend-upgrade-isolated` `release/workshop-v0.1-upgrade-deeplink` | clean cherry-picks of `bfd2d5a3` + `4638f3d7` onto `origin/dev` → tip `c2b85dd3`. |
| Site / download / Clerk | production | external; not frozen here |
| Signed artifact | none yet | Gate F blocker |

See [LAUNCH_READINESS_STATUS.md](./LAUNCH_READINESS_STATUS.md) for the live Gate F/P gap list.

## Integration rules

- Do not commit mixed worktrees wholesale.
- Workshop and evals trees must be clean before every release gate.
- Historical Workshop `8c4eb78` (“correct Luna pricing”) is not trusted by subject line; use annotated follow-up `ffe010d` and the diff.
- Push only after review. This freeze does not authorize publish, notarize, or live payment.

## Hard no-go (either gate)

- Unsigned / un-notarized / checksum-mismatched artifact
- Intern visible on the friend surface
- Starter/Pro not provider-resolved or not enforceable
- Synth Cloud turn with invented or duplicated dollars
- CRAFTAX-LUNA-010 not run against the exact installed candidate
- Secrets in logs, snapshots, or support bundles
- Fixture-only Trace V5 “pass”
