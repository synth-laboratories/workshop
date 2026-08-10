# Handoff: CUA dogfood · design-debt tests · polish

**Date:** 2026-08-09  
**Audience:** Engineer who will **use the real Synth Desktop app**, cross-check against notes + Poolside/Laguna desktop UX, spot issues, **fix or flag them with tests**, and ship small polish — logging every polish item in [`polish.md`](./polish.md).  
**Not** the Intern SDK rewrite or a greenfield feature lane. Prefer tight fix → test → polish loops.

---

## 0. One-liner

> Keep the product honest: **dogfood via CUA**, grow **Playwright / Bombadil / static debt tests**, fix what you can, and **record polish in `polish.md`**. When something is still wrong by design, leave a `test.fail` (or static grep) so the next person can’t forget it.

For the shared visual and interaction bar, use the repo-level [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](../../WORKSHOP_QUALITY_STYLE_GUIDE.md). This handoff describes the dogfood loop and Desktop-specific debt; the guide describes the product language and definition of done.

---

## 1. How you work (loop)

```text
1. Run / install Desktop (canonical scripts — see README)
2. CUA: walk primary surfaces against intended design (below)
3. Cross-ref handoffs + Laguna/Poolside UX notes
4. Either FIX the bug/stub  OR  add/flag a test that fails until fixed
5. Add a row to polish.md for every polish / fix you ship
6. Re-run: npm run test:a11y && playwright design-debt + relevant specs
```

**Rules of thumb**

- Prefer **fixing** toast stubs and inert chrome when the intended behavior is obvious (e.g. Account → Settings Account section).
- Prefer **`test.fail` + static grep** when the fix is large (Intern intervene API, LoRA wiring, MCP dogfood).
- When a `test.fail` starts passing for real → flip to `test(...)` and invert the matching static assert in `design_debt.test.mjs`.
- Do **not** reintroduce LoRA/Finetunes stub UI (`local_lora.md`).
- Do **not** invent container discovery via port scan (`containers.md`).
- Log polish even when “tiny” (copy, spacing, empty states, toasts → real navigation).

---

## 2. Canonical app bring-up

From workshop root ([README](../../README.md)):

```bash
# Laguna (optional but needed for local chat dogfood)
npm run laguna:setup    # once
npm run laguna:serve    # :7333
source ~/.synth-desktop/laguna/env.sh

# Desktop
npm run desktop:dev       # fast loop
# or ship-like:
npm run desktop:install
npm run desktop:status
```

Isolated instances / multiple builds: [`HANDOFF_ISOLATED_DEV_INSTANCES.md`](./HANDOFF_ISOLATED_DEV_INSTANCES.md).

**CUA tip:** Prefer the **installed** `/Applications` app or `desktop:dev` window — not `apps/mock`. Confirm process via `npm run desktop:status`.

---

## 3. Cross-reference map (read before guessing)

| Doc | Use for |
| --- | --- |
| [`testing.md`](../../testing.md) | Suite map, how to run, interpreting failures |
| [`polish.md`](./polish.md) | **Your log** — append every polish/fix |
| [`design-debt.spec.ts`](./tests/playwright/design-debt.spec.ts) | Intended locks + expected-fail debt |
| [`design_debt.test.mjs`](./tests/design_debt.test.mjs) | Static stub / smell greps |
| [`gaps.spec.ts`](./tests/playwright/gaps.spec.ts) | Migration gaps (MCP, Intern w/o Python, SQLite migrate) |
| [`containers.md`](./containers.md) + [`HANDOFF_CONTAINERS_CRAFTAX.md`](./HANDOFF_CONTAINERS_CRAFTAX.md) | Register/hydrate Craftax `:8098` |
| [`HANDOFF_TRACES_V5.md`](./HANDOFF_TRACES_V5.md) + [`TRACES_V5_STORAGE_FORMAT.md`](./TRACES_V5_STORAGE_FORMAT.md) | Ingest → Open → PostTrain |
| [`local_lora.md`](./local_lora.md) | LoRA UI removed until wired — do not restore stubs |
| [`HANDOFF_INTERN_LOCAL_SLOT.md`](./HANDOFF_INTERN_LOCAL_SLOT.md) | Leave-safe, Provide input, local slot honesty |
| [`HANDOFF.md`](./HANDOFF.md) / [`synth_desktop_research_eng.md`](../../synth_desktop_research_eng.md) | Product principles (artifacts, rollouts, visuals) |
| Poolside Laguna S 2.1 blog + [trajectories.poolside.ai](https://trajectories.poolside.ai) | Trace/trajectory inspectability bar |
| Poolside Desktop Assistant (if you have access) | Chrome density, agent multi-slot, trajectory feel — **inspire, don’t clone** |

Laguna **daemon** (our sidecar): `services/laguna-daemon/README.md`.  
Laguna **product UX** (Poolside): treat as reference for polish density and “open the run / trajectory,” not as a requirement to match pixel-for-pixel.

---

## 4. CUA walkthrough checklist

Do this on a real build. File issues as you go (fix or `test.fail`).

### Shell / chrome
- [ ] Sidebar: Chats, Cloud, Inventory, Visuals, Settings  
- [ ] Titlebar: Account / Downloads / Expand — today stubs (`design-debt`)  
- [ ] Terminal `⌘J` works in desktop app (browser fixture only says “desktop app”)  
- [ ] No horizontal overflow; composer stays usable with Visual pane open  

### Local Laguna
- [ ] With `laguna:serve`, composer enables; model menu shows Laguna XS (no LoRA subgroup)  
- [ ] Starting/loading copy — no fake download %  
- [ ] Settings → Models: residency / multi-agent; no adapter placeholder UI  
- [ ] Reload Laguna should eventually call a real reload (debt today)  

### Inventory · Containers
- [ ] Attach → `http://127.0.0.1:8098` (Craftax Rust) after GameBench serve  
- [ ] Probe up/down; expanded info shows hydrated `/info`  
- [ ] See [`HANDOFF_CONTAINERS_CRAFTAX.md`](./HANDOFF_CONTAINERS_CRAFTAX.md)  

### Inventory · Traces
- [ ] Import Trace V5 / Open → PostTrain (or rollout inspector)  
- [ ] See [`HANDOFF_TRACES_V5.md`](./HANDOFF_TRACES_V5.md)  

### Visuals
- [ ] Visuals page list / create / open pane shares one `visual_id` with chat chips  
- [ ] MCP create → chat (still `gaps` / debt)  

### Cloud / Intern
- [ ] Sync send works in demo or with API key  
- [ ] Async leave-safe should be projection-driven (debt: always on)  
- [ ] Provide input / Respond should hit Intern API (debt: stub toast)  

### Honesty checks
- [ ] No Finetunes / Laguna LoRAs stub UI  
- [ ] No claim that remote LoRAs load  
- [ ] Demo fixtures bar: OK for dogfood; don’t let it look like production authority  

---

## 5. Tests you own / extend

| Suite | Command | Your job |
| --- | --- | --- |
| Design debt (Playwright) | `npx playwright test --config apps/synth_desktop/playwright.config.ts tests/playwright/design-debt.spec.ts` | Add locks + `test.fail`s for new smells |
| Design debt (static) | `npm run test:a11y` | Grep stubs / forbid regressions |
| Gaps | same config `gaps.spec.ts` | Flip when migration lands |
| Layout / runtime / visuals | full Playwright | Don’t break; extend for polish regressions |
| Bombadil | `npm run test:bombadil --workspace @synth/synth-desktop` | Optional extra invariant if you find layout bugs |
| Verify umbrella | `npm run desktop:verify` | Before calling a slice done |

**Adding a debt flag**

1. Playwright: `test.fail("… intended behavior …", async ({ page }) => { … assert good end state … })`  
2. Static (if string/smell): assert smell **present** in `design_debt.test.mjs` until fixed  
3. One line in `polish.md` under “Flagged (not fixed yet)” if you only filed the test  

**Fixing a debt flag**

1. Implement the real behavior  
2. Flip `test.fail` → `test`  
3. Invert static grep (assert smell **absent**)  
4. Log under “Shipped” in `polish.md`  

---

## 6. Known debt already flagged (start here)

From `design-debt.spec.ts` / `design_debt.test.mjs` (2026-08-09):

1. Account → should open Settings Account / backend settings  
2. Downloads surface missing  
3. Expand chrome state missing  
4. Always-ask permission menu missing  
5. Set up agent flow missing  
6. Async leave-safe hard-wired `!isSync`  
7. Browser Attach / Open-trace dogfood still fragile without inventory stubs  
8. VisualHost Craftax string heuristics for preview variants  

Migration `gaps.spec.ts`: MCP visual→chat, Intern without Python, legacy SQLite migration.

---

## 7. Polish bar (what “good” looks like)

- **Honest empty states** — say what to attach/import, with the right default URL/path  
- **One click to the real place** — never a toast that says “stub” in a ship build if a real route exists  
- **Trajectory/visual inspectability** — Open Trace / Open Visual feels like Poolside trajectories: scrub, metrics beside steps, no dead ends  
- **Density without dashboard clutter** — match workbench IA (chat + right pane); don’t add card grids for their own sake  
- **Testids** — any new control you expect CUA/Playwright to hit gets a stable `data-testid`  

Frontend design rules for net-new marketing-ish surfaces: follow user frontend rules; inside Desktop, **match existing CSS variables / patterns** in `styles/app.css`.

---

## 8. `polish.md` protocol (required)

Every session that ships UI/UX or debt-test work **appends** to [`polish.md`](./polish.md):

```markdown
### YYYY-MM-DD — <short title>
- **Shipped:** …
- **Tests:** flipped / added …
- **Flagged:** …
- **CUA notes:** …
- **Refs:** Poolside / handoff / issue …
```

If you only dogfood and find nothing, still add a one-liner under “Sessions” so the log shows coverage.

---

## 9. Out of scope (hand off elsewhere)

- Reintroducing LoRA/MLX adapters after v0.1 → `local_lora.md`  
- Rust Intern SDK / deleting Python mailbox → `HANDOFF_RUST_CORE_VISUALS_AND_INTERN.md`  
- Full synth-containers GEPA surface → `containers.md`  
- Replacing Harbor native trajectory authority → evals standard  

You may still **flag** those with tests/docs if CUA hits them.

---

## 10. Definition of done (for a polish slice)

- [ ] CUA path exercised on real Desktop  
- [ ] Fix shipped **or** `test.fail`/static flag added  
- [ ] Relevant Playwright / a11y green  
- [ ] Entry in `polish.md`  
- [ ] No new stub toasts for surfaces that already have a real page  

---

## 11. Suggested first day

1. `npm run desktop:dev` + skim `polish.md` / this handoff  
2. Run `design-debt.spec.ts` + `test:a11y` once  
3. CUA: Account stub → **fix** (wire to Settings → Account) → flip tests → log polish  
4. CUA: Attach Craftax if `:8098` up; else Import Trace fixture → Open  
5. Compare one trajectory open vs Poolside trajectories site — note gaps in `polish.md`  
