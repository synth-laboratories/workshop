# Workshop renderer styles

Three files, in cascade order:

| File | Role |
| --- | --- |
| `tokens.css` | The vocabulary. Space, type, tone, surface, selection, measure. |
| `primitives.css` | The shared component layer. Every page composes `.ws-*`; no page restyles them. |
| `app.css` | Shell geometry and the not-yet-migrated page dialects. Shrinks as pages move. |
| `usage.css` | Data → Usage. The first page to move *out* of `app.css`; token-only. |

Import order in `main.tsx` must be `tokens.css` → `primitives.css` → `app.css`, so
un-migrated page rules keep winning during the transition. Migrated page files load
after `app.css`; a page only earns one once its rules are token-only, which is what
the `lint:app-css` gate is measuring.

## Why this exists

`WORKSHOP_QUALITY_STYLE_GUIDE.md` §3 has prescribed an 8px rhythm and three radii
since 2026-08-09. The baseline was remeasured on 2026-08-19 against the
byte-identical `f64dcf80` release-base stylesheet (not raised by this chart
lane), so the gate continues to reject new literals rather than failing every
unchanged candidate. Measured against `app.css` at 7,678 lines, the codebase had:

- **372** distinct hex colors, against 20 color tokens
- **25** distinct font sizes, nine of them half-pixel
- **18** distinct border radii
- **137** card/row/panel/badge selectors, 45 of them cards

That is an enforcement gap, not a taste gap. The guide named color roles but never
gave anyone a space scale or a type scale to type, so every page invented literals
and became its own dialect. `tokens.css` supplies the missing half.

## The seven rules

1. **One primary per view.** If three things look equally urgent, none of them is.
2. **Buttons say what they do.** Why something can't run is a `.ws-badge` or a
   `.ws-note` beside the control — never the button's own label.
3. **Lists are dividers, not boxes.** Use `.ws-list`. Cards (`.ws-card`) are for a
   handful of distinct objects, never for 24 homogeneous records.
4. **Every color is a token.** No hex literal outside `tokens.css`. This is what
   makes dark theme work; the optimizer section alone shipped 52 literals in 165
   lines and is light-on-light in dark mode today.
5. **Six sizes, eight spaces, four radii.** Between two steps, pick one of the two.
6. **Bound the measure.** `.ws-page` caps its column so a row's action stays beside
   its content instead of flying to the far edge of a wide window.
7. **Density comes from type, not from squeezing whitespace.** Workshop's copy runs
   at 12–13px, and small type needs *more* vertical room. A title and its metadata
   line set 2px apart read as one dense block however correct the values are. A list
   row is `--sp-4`/`--sp-5` in from its edges with `--sp-2` between title and metadata
   (closer to each other than to the neighbouring row, so they group); an
   inspector puts `--sp-5` between regions. Buy density by cutting sizes, not air.

## Migration map

| Today | Becomes |
| --- | --- |
| `.inventory-page` | `.ws-page` |
| `.inventory-head` | `.ws-page-head` |
| `.inventory-list` + `.inventory-row` | `.ws-list` + `.ws-item` |
| `.inventory-tab` | `.ws-tab` |
| `.inventory-empty`, `.optimizer-empty` | `.ws-empty` |
| `.optimizer-launch-card` | `.ws-card.ws-card-split` |
| `.optimizer-toolbar` | `.ws-toolbar` |
| `.optimizer-workbench` | `.ws-workbench` |
| `.optimizer-inspector dl` | `.ws-kv` |
| `.optimizer-status.*` | `.ws-badge-*` |
| `.optimizer-eyebrow` | `.ws-eyebrow` |
| `.primary-button` | `.ws-btn.ws-btn-primary` |
| `.secondary-button`, `.ghost-button`, `.inventory-row-action`, `.trace-inspect-action` | `.ws-btn.ws-btn-secondary` / `.ws-btn-ghost` |

Convert one page at a time and delete its section of `app.css` in the same commit.
Keep every `data-testid` untouched — the CUA suites key on them.

Suggested order: Optimizers (worst offender, most self-contained) → Data → Visuals
→ Connectors. The Traces tab already has the right structure; it becomes
`.ws-panel` + `.ws-list` with columns.

## Making it stick

Add a CI check over `app.css`: no hex literal, no bare `font-size`, no bare
`border-radius`. Without it the counts climb back — the guidance already existed and
the number still reached 372.
