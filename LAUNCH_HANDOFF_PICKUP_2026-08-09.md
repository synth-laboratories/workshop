# Workshop v0.1 launch — pickup handoff

**When:** 2026-08-09 ~23:10 ET  
**Audience:** next person driving launch to a green `gate:release` / `gate:verify`  
**Do not** treat this as a replacement for the launch contract or the original gate burn-down — it is a status delta on top of those.

---

## Canonical refs (read these first)

| Doc | Role |
|---|---|
| [`launch_v0p1.md`](launch_v0p1.md) | **Launch contract** — what v0.1 ships, `[alpha]` / Deferred boundaries, workflows, QA expectations |
| [`LAUNCH_HANDOFF_2026-08-09.md`](LAUNCH_HANDOFF_2026-08-09.md) | **Original v0.1 push / gate burn-down** — blockers as of earlier evening, burn-down order, commands, traps, §6 de-scope protocol |
| [`launch_gate_implementation_plan.md`](launch_gate_implementation_plan.md) | Gate harness design + hardening log |
| [`qa_cua_end_to_end.md`](qa_cua_end_to_end.md) | CUA / manual QA catalog (37 items) |
| `evals/workshop/README.md` | Gate command reference |
| `evals/workshop/manual/CUA_MANUAL_GATE.md` | Manual matrix runbook |
| [`apps/synth_desktop/polish.md`](apps/synth_desktop/polish.md) | Product polish changelog (steer / debt entries at top) |
| [`apps/synth_desktop/HANDOFF_WHISPER_DEPS.md`](apps/synth_desktop/HANDOFF_WHISPER_DEPS.md) | Voice / Whisper env ownership (separate from tonight’s product gate) |

**De-scope rule (unchanged):** if something cannot ship tonight, move the claim to `[alpha]`/Deferred in `launch_v0p1.md` **and** change the matching gate check in the **same** commit. Never green by deleting a check or editing receipt JSON. See §6 in [`LAUNCH_HANDOFF_2026-08-09.md`](LAUNCH_HANDOFF_2026-08-09.md).

---

## Git / checkout status

| Repo | Branch | Tip (product) | Notes |
|---|---|---|---|
| `workshop` | `synth-cloud-provider-desktop` | **`99e91a5`** — *Ship Desktop launch product: steerTurn, visual_manage dogfood, and debt scrub.* | Ahead 1 / **behind 2** vs `origin`. Rebase or merge before push. |
| `evals` | (gate harness under `evals/workshop/`) | Harness still largely **untracked / not landed** as of earlier handoff; confirm before release | Release lane blocks on dirty trees (`SOURCE-CLEAN-*`) |

**Still dirty after `99e91a5` (do not vacuum blindly):**

- `services/laguna-daemon/**` — large concurrent Laguna/MLX WIP  
- Root launch docs still **untracked** (`launch_v0p1.md`, `LAUNCH_HANDOFF_2026-08-09.md`, this file, `qa_cua_end_to_end.md`, …) — **commit these** before release or the contract is not on the revision the gate binds  
- Desktop scripts (`scripts/desktop*.sh`), other `visuals/**` edits  
- Post-commit drift already on tree: `whisper.rs`, `codex.rs`, `lib.rs`, `Composer`/`Settings`/CSS, optimizers skill, etc. — inspect mtimes before staging  

---

## What landed in `99e91a5` (product gate delta)

Clears the **renderer PRODUCT-NO-XFAIL / PRODUCT-NO-LAUNCH-DEBT** slice called out in the original evening handoff.

1. **`steerTurn` (implement, not de-scope)**  
   - Codex app-server `turn/steer` → Rust `CodexManager::steer_turn` / `codex_turn_steer` → `window.synthCodex.steerTurn` → composer `steerSupported` / `onSteer`  
   - Verify: `cargo test --lib steer_turn` (2 tests); Playwright `poolside-polish` steer case  

2. **MCP visual create dogfood (Playwright half)**  
   - `gaps.spec.ts`: Codex emits `synth_visuals.visual_manage` `operation:create` with `structuredContent.visual`; originating chat opens pane **without** calling `synthVisuals.list/get` (registry throws if touched)  
   - **Not covered:** real `synth-visuals-mcp` stdio ↔ `visuals_ipc` TCP — needs a Rust integration test later; do not fake it in Playwright  

3. **Debt scrub**  
   - `analysis.visual.v1` normalizes agent `type`/`text` → `kind`/`body`  
   - Async leave-safe projection-driven (`intern.leaveSafe`); Respond → `intern-intervention-input` via send  
   - Intern fully deferred for v0.1: picker, sidebar, search, CloudDesk, status, and launch-copy entry points are absent; v0.2 re-entry criteria live in `launch_v0p1.md` §4.8  
   - Removed `DemoFixturesBar`; health local mode `stub` → `absent`; stub toast theater scrubbed  
   - Scanner spot-check at handoff write: no `test.fail` / `test.skip` in desktop tests; no stub/demo-fixture hits under `renderer/src`  

Also bundled in that commit (needed for compile / prior session work): chrome honesty, slash/voice surfaces, optimizers UI, workspace scope, Whisper module, preferences, Poolside polish tests, etc.

---

## What’s still blocking launch (vs original handoff)

**2026-08-10 03:40Z update:** `npm run gate:pr -- --workshop /Users/joshuapurtell/Documents/GitHub/workshop` is **GREEN** with 0 blocking failures (109 Playwright, 203 Rust, 67 static, 14 harness tests; Bombadil green). `PRODUCT-NO-INTERN-V0P1` passes: picker/sidebar/search/setup/status surfaces are absent while dormant v0.2 implementation remains. The PR receipt's only two failures are the expected nonblocking dirty-tree findings.

Original evening blockers → **current**:

| Item | Was | Now |
|---|---|---|
| 12 Playwright `test.fail` + 4 launch-debt findings | Blocking `PRODUCT-NO-*` | **Cleared** on tip `99e91a5` (re-run `gate:pr` to confirm receipt) |
| Deterministic PR gate | Previously needed re-run | **GREEN** — product/static/Playwright/Rust/Bombadil/harness checks pass, including full v0.1 Intern removal |
| `LIVE-TRACE-CORRELATION` | Hardcoded fail in `evals/workshop/runner/live.ts` | **Still blocking** — implement driver route **or** §6 de-scope claim + gate together |
| Dirty trees / untracked gate harness | Release blocked | **Still** — land `evals/workshop` harness; commit workshop contract docs + remaining intentional WIP; don’t sweep Laguna into the wrong commit |
| Topology (`gate:preflight`) | `RED_INFRA` | **Still** — free/clean slot, frontend device-init JSON, MLX bearer + auth fail-closed, Craftax, eval driver |
| Signed / notarized artifact | Pending | **Still** — gate verifies codesign / spctl / stapler |
| 37 CUA manual items | Pending | **Still** — `gate:manual:init` only after **final** artifact SHA + workshop revision |

Burn-down from here (same order as original §2, updated):

1. ~~Re-run `gate:pr`.~~ **GREEN** at 2026-08-10 03:40Z, including `PRODUCT-NO-INTERN-V0P1`.  
2. **Decide Trace V5 correlation** (§6 or implement).  
3. **Commit** remaining workshop docs + curated Laguna/desktop WIP; **land evals gate harness**.  
4. Topology → preflight green.  
5. Signed artifact → manual init → 37 CUA → `gate:release` → `gate:verify`.

Commands: unchanged — see §3 of [`LAUNCH_HANDOFF_2026-08-09.md`](LAUNCH_HANDOFF_2026-08-09.md).

---

## Product honesty already in the contract

From `launch_v0p1.md` (Intern / chrome):

- Intern is **fully Deferred to v0.2** and absent from every v0.1 product and launch surface.  
- Legacy Python migration UI removed from Runtime settings.  
- No dead titlebar Account-menu / Expand theater; Downloads → Settings → Models.  

Do not re-open Intern Sync/Async until the v0.2 contract and matching gates are restored per `launch_v0p1.md` §4.8.

---

## Verify quickly (desktop)

```bash
# From workshop root
node --test apps/synth_desktop/tests/design_debt.test.mjs
npx playwright test --config apps/synth_desktop/playwright.config.ts \
  tests/playwright/gaps.spec.ts \
  tests/playwright/design-debt.spec.ts \
  tests/playwright/poolside-polish.spec.ts
cd apps/synth_desktop/src-tauri && cargo test --lib steer_turn
```

```bash
# From evals/workshop — after harness is committed / usable
npm run gate:pr -- --workshop /Users/joshuapurtell/Documents/GitHub/workshop
```

---

## Out of scope for this pickup (parked)

- Real MCP binary ↔ `visuals_ipc` Rust integration dogfood (Playwright cannot reach it).  
- Expanding Bombadil coverage (post-launch polish per original handoff).  
- Whisper download UX: see [`HANDOFF_WHISPER_DEPS.md`](apps/synth_desktop/HANDOFF_WHISPER_DEPS.md) (doc claims dedicated Whisper env; re-verify on a clean machine before trusting Voice download).  

---

## One-line status

**The deterministic PR gate is GREEN and Intern is fully absent for v0.1; launch is now Trace V5 decision + clean commits + topology + notarized artifact + 37 CUA — follow [`LAUNCH_HANDOFF_2026-08-09.md`](LAUNCH_HANDOFF_2026-08-09.md) burn-down from step 2 onward, against the contract in [`launch_v0p1.md`](launch_v0p1.md).**
