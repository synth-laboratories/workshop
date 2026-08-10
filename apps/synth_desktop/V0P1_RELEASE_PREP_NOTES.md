# Workshop v0.1 — release prep notes

**Updated:** 2026-08-10 ~11:00 EDT  
**Audience:** release driver + whoever merges gate/product fixes next  
**Formal launch contract:** [`launch_v0p1.md`](../../launch_v0p1.md) (repo root)  
**Full execution handoff:** [`LAUNCH_V0P1_RELEASE_PREP_HANDOFF.md`](../../LAUNCH_V0P1_RELEASE_PREP_HANDOFF.md)  
**Muse sidecar (not a v0.1 ship blocker unless claimed):** [`muse_sidecar.md`](muse_sidecar.md)

This file is the **living prep board**: what is on `dev`, what still needs merge, known reds, and the next ordered work. The formal handoff still owns the GREEN / 37-CUA / no-go rules.

---

## 1. Outcome (unchanged)

Qualify **one** signed Workshop v0.1 artifact against pinned Workshop + evals revisions.

Releasable only when:

- configured release lane → `GREEN`
- 37/37 manual/CUA items have valid evidence
- public web/download smoke matches the published artifact

A green unit suite, debug `tauri dev`, or unbound checklist is **not** a release decision.

---

## 2. Source snapshot (2026-08-10)

| Tree | Tip | Notes |
| --- | --- | --- |
| `workshop` `dev` / `origin/dev` | `4450204` | Muse Glimmer merge + titlebar trim + connectors/copy |
| `gate/v0.1-product-fixes` worktree | `c3c3b82` on `af1c4ce` | **One merge behind `dev`** — merge/rebase before land |
| `evals/workshop` | untracked / fragile | Gate harness edits may exist only in working tree — pin & commit before any release run |
| Muse worktree | detached `4450204` | Do not relaunch stale muse `tauri dev`; use `/Applications/Synth Desktop.app` |

### On `dev` already (no action)

- Connectors nav removed; message copy actions (`556926d` via compaction merge)
- Titlebar: no Local pill; terminal + right-panel furthest right (`2530d35`)
- Muse Glimmer managed download/spawn/UI (`be7f90d` → `4450204`) — **API parity incomplete**; see `muse_sidecar.md`
- Dock geometry, aria-controls, inference-rail fit, transcript floor, Intern v0.1 surface removal (via earlier landings)
- Readiness audit reported **7/8** with only `RELEASE-ARTIFACT` outstanding (re-check after freeze)

### Waiting to land — `c3c3b82` (`gate/v0.1-product-fixes`)

Parent was `af1c4ce`; rebase/merge onto `4450204` first. Cherry-pickable pieces:

1. **LandingPage.tsx** — picker `maxHeight` clamped to available slot (real overlap bug).
2. **layout.spec.ts** — account control asserts popup trigger (stale always-mounted assumption).
3. **layout.spec.ts** — Connectors control assertion conditional (nav removed).
4. **Bombadil horizons** 5s→8s + `test:bombadil:reasoning` script; pairs with gate **12s** limit.

Evals working-tree (not in workshop git): Bombadil suites added to `runner/suites.ts`, `BOMBADIL_TIME_LIMIT` 5→12s, one-shot hang retry. **Commit/pin evals before release.**

---

## 3. Product surface to qualify (refresh)

vs older handoff — update claims:

| Topic | v0.1 truth |
| --- | --- |
| First run | Continue locally · Sign in to Synth — no setup-agent card |
| Local models | Laguna XS 2.1; Muse Glimmer **only if** claimed and green on Responses — otherwise hide or alpha-gate |
| Remote | OpenRouter GPT 5.6 Luna, OpenRouter Laguna S 2.1, Synth Cloud Laguna S 2.1 |
| Intern | Absent from every reachable v0.1 surface / claim |
| Sidebar | Chats, Search, Research → Visuals/Optimizers, Inventory → Containers · Traces · Usage, Settings — **no Connectors nav** |
| Titlebar | Terminal + inference panel (when local model); **no** Local pill / account / Models chrome |
| Panes | `ContainerPane`; visuals + Trace V5 share `VisualPane` |
| Credentials | Rust-owned Synth key in desktop `0600` env — not OS keychain |

---

## 4. Hard blockers before `gate:release`

Ordered by dependency:

| # | Blocker | Owner hint |
| --- | --- | --- |
| 1 | Merge `c3c3b82` onto current `dev` (resolve `layout.spec.ts` vs titlebar/connectors reality) | desktop |
| 2 | Pin & commit `evals/workshop` harness (Bombadil suites + 12s limit + hang retry) | evals |
| 3 | Static suite / Intern assertion vs contract (`a11y_surface` vs dormant `nativeIntern.createSession`) — align assert **or** remove dormant call | desktop |
| 4 | Bombadil layout backlog (exit-on-violation, one at a time): inference rail inset; transcript clears composer; composer usable @ 960×640 + visual; modal focus | desktop |
| 5 | Playwright known-red: optimizer-banking77 ×3; poolside-polish; runtime-regressions mid-chat provider switch — fix or document as non-waivable | desktop |
| 6 | **Trace V5 live correlation** — `LIVE-TRACE-CORRELATION` hardcoded fail; need eval-driver join of Craftax rollout data ↔ model/tool events | evals + containers |
| 7 | Clean live topology (slot, frontend/Clerk, MLX, Craftax) + provider-parity coding grade | release room |
| 8 | Signed/notarized artifact + `RELEASE-ARTIFACT-SIGNATURE` format audit | desktop |
| 9 | 37/37 CUA receipt + independent review | CUA + reviewer |
| 10 | Production web/download smoke | web |

**Muse:** not required for v0.1 GREEN unless marketing/picker claims it as a working local target. If claimed → `muse_sidecar.md` parity must be GREEN for Responses through `:7333`. Prefer hide/ungate until then.

---

## 5. Known-red (verified on pristine `dev`, not caused by gate branch)

- **Static:** `a11y_surface.test.mjs` forbids `nativeIntern.createSession` while App still contains dormant path; contract allows dormant for v0.2 — decide deliberately.
- **Bombadil (after gate branch):**  
  - `inference_rail_keeps_a_contained_inset_panel` (~3/6)  
  - `transcript_content_clears_composer` (~2/6)  
  - `composer_remains_usable` at 960×640 with visual  
  - `active_modal_never_traps_or_loses_focus`
- **Playwright:** optimizer-banking77 ×3; poolside-polish ~L117; runtime-regressions “changing providers mid-chat”.

### Bombadil timing (this Mac)

| Limit | Horizon | Wedge rate (n=6) |
| --- | --- | --- |
| 5s | — | rare |
| 10s | — | 1/6 |
| **12s** | **8s** | **0/6** ← settled |
| 20s | — | 3/6 |

A liveness horizon **≥** time limit can never pass — that combo fails everything.

---

## 6. Immediate merge / cleanup checklist

```bash
# 1) Land gate fixes onto current dev
cd /Users/joshuapurtell/Documents/GitHub/workshop
git fetch origin
git checkout dev && git pull
git merge --no-ff gate/v0.1-product-fixes   # or cherry-pick c3c3b82 after rebase onto 4450204
# resolve layout.spec.ts if needed; npm run desktop:verify:fast
git push origin dev
npm run desktop:install && npm run desktop:restart

# 2) After merge, remove gate worktree
git worktree remove ../workshop-gate-fixes
git branch -D gate/v0.1-product-fixes

# 3) Pin evals (critical — harness was untracked)
cd /Users/joshuapurtell/Documents/GitHub/evals/workshop
git status
# commit suites.ts / BOMBADIL_TIME_LIMIT / hang-retry; push; record SHA in release room

# 4) Optional prune
# rm -rf …/scratchpad/wt-baseline
# git worktree prune
```

**Gotchas:**

- Worktree + symlinked `node_modules` → Vite builds the **wrong** tree. Real `npm install` in any worktree you test.
- Do not open `Synth Desktop · muse-glimmer` debug instances stuck behind `dev`; they look like product regressions.

---

## 7. Release execution (abbrev — full cmds in formal handoff)

1. Freeze Workshop + evals SHAs; clean trees  
2. `evals` test / typecheck / `gate:negative-control`  
3. `desktop:check|verify|build` + `gate:pr`  
4. Configured `gate:preflight` (read-only)  
5. Close Trace V5 + artifact-signature blockers  
6. `gate:local` (coding both paths, Craftax ×2, Trace V5, cleanup)  
7. `desktop:install:release` + SHA-256 + sign/notarize  
8. `gate:manual:init` → 37 CUA evidence  
9. `gate:release` + `gate:verify` → both `GREEN`  
10. Production smoke + publish  

No-go rules: dirty tree, hardcoded Trace V5, failed Synth/MLX grading, signing failure, unbound manual item, marketing claim the artifact lacks — see formal handoff §6.

---

## 8. Open task board

| Status | Task |
| --- | --- |
| next | Merge `c3c3b82` onto `4450204` / push `dev` / reinstall |
| next | Commit & pin `evals/workshop` harness |
| open | Bombadil layout violation backlog |
| open | Static Intern assertion vs dormant code |
| open | Playwright known-red triage |
| open | Trace V5 correlation (eval-driver + live check) |
| wip | Live topology: **slot1 `:41109` Laguna S Responses OK** (`openrouter/poolside/laguna-s-2.1`); Desktop configs → slot1; MLX `:7333` + Craftax `:8098` still down; hosted api-dev/prod catalog ≠ Desktop model id — see `synth_cloud_api_usage.md` |
| open | Release artifact + `gate:release` |
| open | 37 CUA + independent review |
| later | Muse first-class sidecar (`muse_sidecar.md`) — only if claimed in v0.1 |

---

## 9. Evidence index (must exist in release room)

- Workshop SHA, evals SHA, frontend/Clerk, slot, MLX, Craftax, artifact SHA + signing  
- `gate:pr` / preflight / `gate:local` / `gate:release` / `gate:verify` receipts  
- 37-item manual receipt + hashed evidence  
- Provider-parity workspaces; Craftax rollout IDs/seeds/frames; Trace V5 join proof  
- Known limitations + rollback artifact  

---

## 10. Decision

**NO-GO** until an exact release artifact has a fresh verified `GREEN` receipt and 37/37 evidence-backed manual checks.

Muse picker presence without Responses parity through `:7333` is a **product honesty** risk — hide or alpha until `muse_sidecar.md` acceptance lands if v0.1 claims local Muse.
