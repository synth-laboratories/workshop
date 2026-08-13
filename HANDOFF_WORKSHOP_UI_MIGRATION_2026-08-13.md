# Handoff — wire the style layer into Optimizers and Data

**Date:** 2026-08-13
**Repo/branch:** `workshop` @ `josh/aug12-optimizers-workshop-visuals`
**Status:** foundation built, committed, and **not wired to anything**
**Scope:** two pages. Not a stylesheet rewrite.

## Why this exists

The Optimizers and Data pages look bad, and it is not a taste problem. There is
nothing in the codebase that can say no. `WORKSHOP_QUALITY_STYLE_GUIDE.md` §3 has
prescribed an 8px rhythm and three radii since 2026-08-09, but `app.css` gave page
authors only color roles — no space scale, no type scale — so everyone typed
literals and every page became its own dialect.

Measured in `app.css` this morning, and again this afternoon:

| | 2026-08-13 10:00 | 2026-08-13 15:07 |
|---|---:|---:|
| lines | 7,678 | 7,711 |
| distinct hex colors | 372 | **394** |
| distinct font sizes | 25 | 26 |
| distinct border radii | 18 | 18 |

**The hex count rose by 22 in one working day.** That is the argument for step 4
below, and it is why the guide alone has not worked.

## What is already done

Committed and present at `apps/synth_desktop/src/renderer/src/styles/`:

| file | what it is |
|---|---|
| `tokens.css` | the missing scale — 8 spaces, 6 type sizes, 4 radii, 6 status tone pairs, surface/selection roles, all themed for dark |
| `primitives.css` | the shared component layer — `.ws-page`, `.ws-card`, `.ws-list`/`.ws-item`, `.ws-btn` tiers, `.ws-badge`, `.ws-tabs`, `.ws-toolbar`, `.ws-kv`, `.ws-note`, `.ws-empty`, `.ws-workbench`, `.ws-dialog` |
| `README.md` | **the seven rules and the full class-by-class migration map.** Read this first; it is not duplicated here. |

Design rationale and before/after renderings of both pages:
<https://claude.ai/code/artifact/da518c0a-5c59-4241-843a-eca32698f463>

## What is not done

**Nothing imports it.** `main.tsx:5` is still only:

```ts
import "./styles/app.css";
```

No page has been migrated. `app.css` still carries 91 `.optimizer*` selectors.

## The work

### 1. Import, in this order

```ts
// apps/synth_desktop/src/renderer/src/main.tsx
import "./styles/tokens.css";
import "./styles/primitives.css";
import "./styles/app.css";   // last, so un-migrated pages still win
```

Order matters. Page rules must keep winning until their page is converted, or you
will half-restyle every screen at once.

### 2. Optimizers first

`components/OptimizersPage.tsx` — worst offender, most self-contained. Convert the
JSX to `.ws-*` per the map in `styles/README.md`, then **delete the
`/* ── Optimizer workbench ── */` block from `app.css` in the same commit**. That
block is ~110 lines and 52 hardcoded colors; leaving it behind is how you end up
with two systems instead of one.

Five defects to fix while you are in there — all visible in the artifact:

1. **Status is rendered as a primary button.** *Beta not configured*, *Recipe
   unavailable* etc. are disabled `.primary-button`s, so the loudest things on the
   page are the three you cannot do, at three different widths. Unavailability is a
   `.ws-badge` next to a plain disabled control.
2. **Six primaries**, three of which restate the recipe cards' own actions in the
   toolbar. One primary per view; page-level actions move to `.ws-page-head-actions`.
3. **Selection is purple** (`#7663ba`, `#6654a9`, `#d8d1ee`, `#f7f4ff`) while
   buttons are blue and the brand is orange. Use `--selected-*`; retire the purples.
4. **Dark theme is already broken here** — 52 literals in 165 lines do not move
   when the theme does.
5. The four recipe cards touch: `.inventory-page` sets no gap and
   `.optimizer-launch-card` sets no margin.

### 3. Then Data

`components/DataPage.tsx`. The Visuals tab renders 24 near-identical records as 24
separately bordered boxes with no width bound, so on a wide window each row's
*Open* button sits ~1500px from its title. `.ws-list` + `.ws-item` inside
`.ws-page` fixes both — dividers instead of per-row borders, and a bounded measure.

Note the Traces tab already has the right structure (table shell, header row,
badges). It becomes `.ws-panel` + `.ws-list` and stops being a bespoke dialect.

### 4. The lint, or this reverts

Add a CI check over `app.css`: no hex literal, no bare `font-size`, no bare
`border-radius`. Without it the numbers climb back — 372 → 394 in a single day is
the evidence. This is the step that makes the other three durable.

## Constraints

- **Preserve every `data-testid`.** 38 in OptimizersPage, 19 in DataPage. The CUA
  and Playwright suites key on them: `tests/playwright/optimizer-banking77.spec.ts`,
  `visual-responsive-gate.spec.ts`, `design-debt.spec.ts`, `poolside-polish.spec.ts`,
  `tests/bombadil/layout.spec.ts`, `trace-catalog-layout.spec.ts`,
  `shell-containment.spec.ts`, `tests/a11y_surface.test.mjs`.
- **No new prose in the UI.** Trim, do not write. Standing rule.
- **Check mtimes before editing.** Both pages were being written by another agent
  earlier today (`OptimizersPage.tsx` at 09:59, `app.css` at 12:41). They are quiet
  now, but confirm before you start — this repo has concurrent writers.
- `app.css` also has uncommitted changes from other people. Do not sweep them into
  your commit; stage by path.

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

- `tokens.css` and `primitives.css` imported ahead of `app.css`
- Optimizers and Data rendering from `.ws-*`, with their `app.css` blocks deleted
- Both pages legible in dark theme (they are not today)
- Every `data-testid` intact; UI gates no worse than baseline
- A lint that fails on a new hex literal in `app.css`
- `app.css` hex count **down**, not up

## Do not

- Rewrite `app.css` wholesale. Convert one page, delete that page's block, repeat.
- Add a second token system. `--sv-*` in `visuals/chrome/tokens.css` is for artifact
  surfaces and is deliberately separate; `--ws-*`/`--sp-*`/`--fs-*` are the desktop.
- Restyle `.ws-*` from a page. If a page needs a variant, add a modifier to
  `primitives.css` so the next page inherits it.
