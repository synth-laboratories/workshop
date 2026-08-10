# Synth Workshop visual style guide v0.1

## Purpose

This guide defines the initial shared visual language for Synth Workshop. It is intentionally narrow: it standardizes the foundations and reusable patterns that create the largest consistency improvement without requiring a complete redesign.

The rules are normative for new UI. Existing UI should move toward them as each surface is touched.

## Product character

Synth should feel quiet, capable, technical, and native to a desktop work environment.

- Prefer clarity over decoration.
- Keep frequent workflows compact without making text difficult to read.
- Use emphasis sparingly so active work, warnings, and primary actions remain meaningful.
- Reveal implementation detail only when it helps the user make a decision.
- Use the same visual grammar across chat, settings, inventory, models, traces, visuals, and supporting panels.

## Initial consistency contract

The following rules apply throughout the app in v0.1:

1. Use the shared typography scale.
2. Use the 4px spacing scale.
3. Use semantic color tokens; do not hard-code light-theme colors in components.
4. Use only the approved radius and elevation levels.
5. Use the shared control heights and state treatments.
6. Communicate selection with an indicator in addition to color.
7. Use one page shell and one active navigation context at a time.
8. Constrain long-form content to a readable maximum width.
9. Use sentence case for UI labels and headings.
10. Preserve visible keyboard focus and reduced-motion behavior.

## Foundations

### Typography

Use `--font-sans` for interface copy and the user-selected monospace family for code, commands, paths, logs, identifiers, and numeric data that benefits from alignment. A font-family preference control should display a friendly family name rather than a CSS fallback stack.

| Role | Size / line height | Weight | Use |
|---|---:|---:|---|
| Page title | 18 / 24px | 650 | One title per page or major pane |
| Section title | 14 / 20px | 650 | Major groups within a page |
| Row or card title | 13 / 18px | 600 | Setting names, list-item titles |
| Body | 13 / 20px | 400 | Explanations and primary reading text |
| Label | 12 / 16px | 600 | Form labels and compact controls |
| Helper text | 12 / 18px | 400 | Secondary descriptions and guidance |
| Metadata | 11 / 16px | 500 | Timestamps, status detail, provenance |
| Code compact | 11 / 16px | 400 | Dense paths, commands, identifiers |

Avoid interface text below 11px. Specialized visualizations may use 10px labels only when space is intrinsically constrained and the same information is available elsewhere.

Use negative letter spacing only on page titles. Use uppercase only for short metadata kickers; never use it for paragraphs, buttons, or form labels.

### Spacing

Use a 4px base grid.

| Token | Value | Typical use |
|---|---:|---|
| `--space-1` | 4px | Icon/text micro-gap |
| `--space-2` | 8px | Related controls and compact rows |
| `--space-3` | 12px | Field padding and card internals |
| `--space-4` | 16px | Standard component separation |
| `--space-6` | 24px | Page padding and section separation |
| `--space-8` | 32px | Separation between major page regions |

Do not introduce intermediate spacing values without a specific layout requirement. Prefer `gap` on the parent over margins distributed among children.

### Color

Components must consume semantic variables so light and dark themes can share behavior. The existing Synth orange remains the brand and primary-action accent, but it must not also carry warning or error meaning.

Required semantic roles:

```css
:root {
  --color-surface: #ffffff;
  --color-surface-subtle: #f3f5f8;
  --color-surface-raised: #ffffff;
  --color-border: #e1e4e6;
  --color-border-strong: #c9ced4;

  --color-text: #242425;
  --color-text-muted: #6b6f76;
  --color-text-faint: #858b94;

  --color-accent: #f05f22;
  --color-accent-hover: #d9541e;
  --color-selection-bg: rgba(240, 95, 34, 0.08);
  --color-selection-border: rgba(240, 95, 34, 0.42);
  --color-focus-ring: rgba(240, 95, 34, 0.35);

  --color-success: #2f8f5b;
  --color-warning: #a86d16;
  --color-danger: #b33a45;
  --color-info: #447dfc;
}
```

The dark theme must override these roles rather than requiring component-specific dark selectors. Avoid `#fff`, `#fafbfc`, or other fixed surface colors inside component rules.

Color usage rules:

- Orange identifies brand, focus, selection, and the primary action—not warnings.
- Red is reserved for destructive actions, failures, and invalid values.
- Amber is reserved for warnings, degraded states, and decisions requiring caution.
- Green is reserved for confirmed success or healthy/ready states.
- Blue is reserved for links, information, and external navigation where appropriate.
- Do not use color as the only carrier of status or selection.

### Radius

Use four radius levels:

| Token | Value | Use |
|---|---:|---|
| `--radius-xs` | 4px | Keycaps, compact code labels |
| `--radius-sm` | 8px | Buttons, inputs, nav rows, compact controls |
| `--radius-md` | 12px | Cards, menus, popovers, grouped controls |
| `--radius-lg` | 16px | Dialogs, composers, major floating surfaces |
| `--radius-pill` | 999px | Status pills and true capsules only |

Do not add nearby one-off radii such as 7, 9, 10, 11, 13, or 14px. Selected navigation rows should not use a larger radius or shadow than unselected rows.

### Borders and elevation

- Use a 1px solid `--color-border` boundary for ordinary controls and in-flow cards.
- Use `--color-border-strong` only when a stronger separation is required.
- Reserve dashed borders for drop zones or incomplete placeholders.
- Do not use shadows on ordinary rows, settings selections, or nested cards.
- Use `--shadow-sm` for menus and small floating controls.
- Use `--shadow-md` for dialogs and major overlays.
- Prefer a border or surface change to a shadow when the element remains in document flow.

### Icons

- Use 16px icons inside standard controls and navigation rows.
- Use 20px icons for major pane actions or empty-state illustrations.
- Keep stroke weight and optical size consistent within a surface.
- Pair unfamiliar icons with text or an accessible tooltip.
- Do not mix emoji, text glyphs, and line icons for equivalent actions.

### Motion

- Standard state transition: 120–160ms using `--ease-out`.
- Panels and larger overlays: up to 200ms.
- Avoid decorative continuous animation. Reserve it for real running or loading states.
- Disable nonessential motion under `prefers-reduced-motion: reduce`.

## Layout

### Application and page shells

- Show one active navigation context at a time. A settings page must replace the conversation sidebar or open in its own settings shell; it must not introduce a second full sidebar beside the first.
- Use one compact page header. Avoid stacking a tab title, back label, page title, breadcrumb, and repeated section title.
- Keep global actions in the application chrome and page-specific actions in the page header.
- Do not leave unrelated model, account, or conversation status visually prominent on focused settings and management surfaces.

### Content width

- Standard pages: `max-width: 960px`.
- Reading-heavy text: `max-width: 68ch`.
- Helper text: generally `max-width: 52–62ch`.
- Center a constrained page when the surrounding pane is wider than the maximum.
- Use 24px page padding on standard desktop widths and 16px at narrow widths.

### Responsive grids

- Use three columns only when each field can retain its useful minimum width.
- Collapse three columns to two before any value truncates or label wraps awkwardly.
- Collapse to one column below approximately 760px of available content width.
- A numeric field should not expand merely to fill a grid column; use intrinsic control widths.
- Long text fields and selectors may span the full row.

## Reusable component patterns

### Page header

A page header contains:

- one page title;
- an optional one-sentence description;
- up to one primary action and a small number of secondary actions.

Use a breadcrumb only when the user has traversed more than one level. Do not repeat the same page name in both the breadcrumb and title.

### Section

A section contains a section title, optional helper text, and one coherent group of rows or controls. Separate major sections by 24–32px or a divider, not both unless the page is unusually dense.

### Settings row

Prefer a Codex-style settings row for a single value or switch:

- label and explanation on the left;
- control aligned right;
- 12–16px internal padding;
- divider between adjacent rows;
- full card boundary around the group, not around every row.

At narrow widths, place the control below the copy and align it left.

### Choice group

Use a segmented control for two to four short labels. Use a radio list when options require explanations.

- Show a radio/check indicator for the selected option.
- Use `--color-selection-bg` and `--color-selection-border` for selection.
- Do not use warning or error styling for ordinary selection.
- Keep each option compact; a one-line choice should not become a large full-width card.

### Model and capability pickers

Model selection must follow progressive disclosure: show the information needed for the current decision and reveal technical detail only on request.

The closed composer control shows only:

- the current model name;
- an optional model icon;
- at most one short adjacent value, such as effort or thinking state;
- a disclosure chevron.

The first menu presents adjustable dimensions rather than a catalog dump:

```text
Model       Laguna XS 2.1  >
Thinking    On             >
Speed       Fast           >
----------------------------
Advanced                    ^
```

The model submenu contains one compact row per model:

```text
Laguna XS 2.1              check
Muse Glimmer 30B
GPT 5.6 Luna
Laguna S 2.1
```

Rules:

- Default model rows contain the user-facing model name and selected checkmark only.
- Do not expose runtime, framework, provider ID, billing status, modality, and context limit in every row.
- Do not concatenate capability values into strings such as `Text only262K context`.
- Put technical metadata in Advanced, an inspector, a tooltip, or a detail area for the currently highlighted/selected model.
- Reveal metadata in a stable labeled layout; do not add an unpredictable third line to individual rows.
- If two entries share the same user-facing model name, prefer one model choice with routing/provider under Advanced. If the provider materially changes the decision, add one short disambiguator such as `Local` or `OpenRouter`—not a full metadata sentence.
- Modality uses plain text or a correct icon plus text. Never place an image icon beside `Text only`.
- Keep rows compact and consistent in height.
- Constrain the menu to the available viewport and scroll the catalog internally.
- Keep the selected checkmark in a fixed trailing column.

The principle is: **design for the decision; progressively disclose the metadata**.

### Cards

Use cards for meaningful boundaries: summaries, previews, independent resources, or content with its own actions. Do not wrap every section, row, and nested subgroup in separate cards.

Standard card treatment:

- 12px radius;
- 1px border;
- no shadow while in flow;
- 12–16px padding;
- title, optional metadata, body, then actions.

### Inputs and selects

- Default control height: 36px.
- Compact toolbar control height: 32px.
- Major primary action or high-confidence touch target: 40px.
- Numeric control width: 88–110px unless the expected value requires more.
- Short select width: 160px.
- Standard select/text field width: 240–320px.
- Long paths or technical values may be fluid and span the row.

Controls use sans-serif by default. Apply monospace only to a value that is genuinely code-like.

### Buttons

- Primary: filled accent; one primary action per local surface.
- Secondary: neutral border and surface.
- Ghost: no persistent border; use for low-emphasis toolbar actions.
- Danger: neutral by default when reversible, red when destructive intent must be unmistakable.
- Icon-only buttons require a tooltip and accessible name.

### Navigation rows

- Standard height: 36px minimum.
- Use an 8px radius and consistent icon slot.
- Hover uses a subtle neutral background.
- Selected uses a stronger neutral or selection background plus weight or an indicator.
- Focus uses the shared focus ring and must remain distinct from selected state.
- Never show two selected navigation destinations simultaneously.

### Status and feedback

- Pair status color with text, icon, shape, or state language.
- Distinguish running, queued, ready, warning, failed, and unavailable states.
- Use inline validation next to the affected control.
- Use banners for page-level warnings and toasts for transient confirmation.
- Do not present an unknown state as healthy merely because the last known status was healthy.

### Empty states

An empty state contains one concise explanation and one useful next action. Avoid oversized decoration, repeated instructions, and inactive placeholder actions.

### Keyboard shortcuts

Render shortcuts as keycaps using the platform’s terminology and symbols. Keep formatting consistent: for example, `⌘` + `Enter`, not a mixture of `Cmd+Enter`, `⌘Enter`, and `Command + Enter`.

## Content style

- Use sentence case: “Prompt submission,” not “Prompt Submission.”
- Prefer short nouns for navigation and verb phrases for actions.
- Button labels should describe the result: “Save as default,” not “Apply.”
- Helper text should explain consequences or constraints, not restate the label.
- Use user-facing names instead of implementation names or raw configuration values.
- Keep destructive labels explicit: “Delete conversation,” not “Remove.”
- Use an ellipsis only when an action opens a flow that requires more input.

## Accessibility requirements

- Maintain at least 4.5:1 contrast for normal text and 3:1 for large text and meaningful UI boundaries.
- All interactive elements need a visible `:focus-visible` state.
- Target at least 32×32px for compact desktop controls and 36×36px where space permits.
- Selection and status must not depend on color alone.
- Preserve logical keyboard order and semantic HTML controls.
- Respect text resizing without clipping values or overlapping controls.
- Test light, dark, high-contrast, and reduced-motion behavior.

## Implementation rules

- New components must use shared tokens rather than new literal colors, spacing values, font sizes, shadows, or radii.
- If the token set cannot express a legitimate need, add a semantic token rather than a component-specific literal.
- Shared patterns should become reusable primitives before they are copied into a third surface.
- Avoid selectors that make dark mode a component-by-component patch.
- Visual refactors must preserve semantic controls, accessible names, and keyboard behavior.

Recommended first primitives:

- `PageShell`
- `PageHeader`
- `Section`
- `SettingsGroup`
- `SettingsRow`
- `ChoiceGroup`
- `Field`
- `Button`
- `IconButton`
- `Badge`
- `StatusDot`
- `EmptyState`

## Adoption order

1. Add the missing semantic, spacing, typography, and radius tokens.
2. Build the shared settings/page primitives.
3. Apply them to Settings General and remove the nested-sidebar layout.
4. Apply the same page, row, field, and state patterns to Models, Account, Runtime, and Inference.
5. Consolidate recurring cards, buttons, badges, empty states, and pane headers across Inventory, Traces, Visuals, and Optimizers.
6. Remove obsolete one-off literals after each migrated surface is visually verified in light and dark themes.

## Review checklist

Before merging a new or substantially changed surface, verify:

- It has one navigation context and one page title.
- Typography, spacing, radii, colors, and shadows use approved tokens.
- The content remains readable at wide, standard, and narrow window widths.
- Controls are sized for their content rather than their grid column.
- Hover, selected, focus, disabled, loading, error, and empty states are visually distinct.
- Selection and status are not communicated by color alone.
- Text does not clip at the supported font-size range.
- Light theme, dark theme, keyboard navigation, and reduced motion were checked.
