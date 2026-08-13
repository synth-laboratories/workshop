# Handoff — wire the style layer into Optimizers and Data

**Date:** 2026-08-13 (revised evening — half of this landed, half was reverted)
**Repo/branch:** `workshop` @ `josh/aug12-optimizers-workshop-visuals` (`8ed2613`)
**Scope:** one page, plus the lint that keeps it. Not a stylesheet rewrite.

## Why this exists

The Optimizers and Data pages look bad, and it is not a taste problem. There was
nothing in the codebase that could say no. `WORKSHOP_QUALITY_STYLE_GUIDE.md` §3 has
prescribed an 8px rhythm and three radii since 2026-08-09, but `app.css` gave page
authors only color roles — no space scale, no type scale — so everyone typed
literals and every page became its own dialect.

## Status — read this before starting

Since this handoff was first written, work landed. Two of the four steps are done,
and **one of them was undone again**. Current state, measured at `8ed2613`:

| step | state |
|---|---|
| 1. import the layer | **done** — `main.tsx:5-7`, correct order |
| 2. Optimizers | **landed at `e25ff5a`, then reverted at `cae6fc3`** — see below |
| 3. Data | **done** — 103 `.ws-*` usages in `DataPage.tsx` |
| 4. the lint | **not done** — and this is why step 2 came back |

`app.css` entropy is down but not resolved:

| | 10:00 | 15:07 | now |
|---|---:|---:|---:|
| lines | 7,678 | 7,711 | 7,466 |
| distinct hex colors | 372 | 394 | **357** |
| distinct font sizes | 25 | 26 | 26 |
| distinct border radii | 18 | 18 | 18 |

### The Optimizers migration was reverted, and that is the main lesson

Traced commit by commit on `OptimizersPage.tsx`:

| commit | `ws-` usages | `className="optimizer*` |
|---|---:|---:|
| `e25ff5a` migrate optimizer and data pages | **97** | 0 |
| `83e2a8f` demote unavailable launch action | 97 | 0 |
| `cae6fc3` **make optimizer workspace run-first** | **0** | 47 |
| `9dd326e` make optimizer setup agent-guided | 0 | 29 |
| `8ed2613` (HEAD) | **0** | 29 |

A concurrent redesign rewrote the page from scratch and reintroduced the bespoke
classes; `app.css` now carries 91 `.optimizer*` selectors again. Nobody did
anything wrong — there was no signal that the page had been converted and no check
that failed when it was unconverted.

**So do step 4 first this time.** A migration that only lives in one page's JSX
survives exactly until the next person redesigns that page.

Design rationale and before/after renderings of both pages:
<https://claude.ai/code/artifact/da518c0a-5c59-4241-843a-eca32698f463>

## What the foundation gives you

Committed at `apps/synth_desktop/src/renderer/src/styles/`:

| file | what it is |
|---|---|
| `tokens.css` | the missing scale — 8 spaces, 6 type sizes, 4 radii, 6 status tone pairs, surface/selection roles, all themed for dark |
| `primitives.css` | the shared component layer — `.ws-page`, `.ws-card`, `.ws-list`/`.ws-item`, `.ws-btn` tiers, `.ws-badge`, `.ws-tabs`, `.ws-toolbar`, `.ws-kv`, `.ws-note`, `.ws-empty`, `.ws-workbench`, `.ws-dialog` |
| `README.md` | **the seven rules and the full class-by-class migration map.** Read this first; it is not duplicated here. |

`DataPage.tsx` at HEAD is the worked example — read it before converting Optimizers.

## The work

### 1. The lint — do this first

Add a CI check over `app.css`: no hex literal, no bare `font-size`, no bare
`border-radius`, and no new `.optimizer*` selector. Without it the numbers climb
back and conversions get overwritten — 372 → 394 in one day, then a completed
migration reverted within hours, are both evidence.

Seed the allowlist at today's counts (357 / 26 / 18) and ratchet down. A check that
demands zero on day one gets disabled.

### 2. Optimizers, again

`components/OptimizersPage.tsx` — 29 `className="optimizer*` sites, 0 `.ws-*`.
Convert per the map in `styles/README.md`, then **delete the
`/* ── Optimizer workbench ── */` block from `app.css` in the same commit**. That
block is ~110 lines and 52 hardcoded colors; leaving it behind is how you end up
with two systems instead of one.

`e25ff5a` is a working reference implementation of this exact conversion — read it
(`git show e25ff5a -- .../OptimizersPage.tsx`) before rewriting from scratch. It
does not apply cleanly onto the run-first redesign, but the class mapping it chose
is the one to reuse.

Defects to fix while you are in there:

1. ~~**Status rendered as a primary button.**~~ Fixed at `83e2a8f`; the demotion
   survived the revert. Leave it demoted.
2. **Multiple primaries**, some restating the recipe cards' own actions in the
   toolbar. One primary per view; page-level actions move to `.ws-page-head-actions`.
3. **Selection is purple** (`#7663ba`, `#6654a9`, `#d8d1ee`, `#f7f4ff`) while
   buttons are blue and the brand is orange. Use `--selected-*`; retire the purples.
   Seven of these literals remain in `app.css`.
4. **Dark theme is broken here** — the literals do not move when the theme does.
5. The recipe cards touch: `.inventory-page` sets no gap and
   `.optimizer-launch-card` sets no margin.

### 3. Data — done, verify only

`DataPage.tsx` renders from `.ws-*` (103 usages). 11 legacy class references remain;
clean them up opportunistically, but this page is no longer the problem.

## Constraints

- **Preserve every `data-testid`.** 19 in OptimizersPage, 18 in DataPage today
  (down from 38/19 because of the run-first redesign, not because any were lost —
  every id referenced by a test still resolves; I checked). The CUA and Playwright
  suites key on them: `tests/playwright/optimizer-banking77.spec.ts`,
  `visual-responsive-gate.spec.ts`, `design-debt.spec.ts`, `poolside-polish.spec.ts`,
  `tests/bombadil/layout.spec.ts`, `trace-catalog-layout.spec.ts`,
  `shell-containment.spec.ts`, `tests/a11y_surface.test.mjs`.
- **No new prose in the UI.** Trim, do not write. Standing rule.
- **Check mtimes and recent commits before editing.** This repo has concurrent
  writers — that is literally what reverted step 2. `git log --oneline -5 -- <file>`
  before you start.
- `app.css` may carry uncommitted changes from other people. Stage by path.

## Gates

```bash
cd apps/synth_desktop
npm run typecheck
npm run test:ui-gates          # bombadil + playwright
npm run test:playwright
```

Baseline failures at `73dbb6f` are pre-existing and not yours — diff against a
clean run before assuming you broke something.

## Done means

- A lint that fails on a new hex literal or a new `.optimizer*` selector in `app.css`
- Optimizers rendering from `.ws-*`, with its `app.css` block deleted
- Both pages legible in dark theme
- Every `data-testid` intact; UI gates no worse than baseline
- `app.css` hex count **below 357**

## Do not

- Rewrite `app.css` wholesale. Convert one page, delete that page's block, repeat.
- Add a second token system. `--sv-*` in `visuals/chrome/tokens.css` is for artifact
  surfaces and is deliberately separate; `--ws-*`/`--sp-*`/`--fs-*` are the desktop.
- Restyle `.ws-*` from a page. If a page needs a variant, add a modifier to
  `primitives.css` so the next page inherits it.
- Land the conversion without the lint. It was already tried; it lasted three commits.
