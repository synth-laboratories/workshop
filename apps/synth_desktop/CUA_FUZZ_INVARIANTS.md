# CUA fuzz findings and regression invariants

**Status:** active, 2026-08-09. This is the durable record for visual defects
found while exercising the installed Synth Desktop build with Computer Use.
Each entry must be fixed, explicitly deferred with an expected-fail test, or
removed from the list only after the stated proof runs.

## Observed failures

| Surface | CUA finding | Product invariant | Proof |
| --- | --- | --- | --- |
| Active transcript | A long pasted user brief became a screen-height blue surface; the newest turn could appear under the floating composer. | A long prompt is compact by default, can be expanded without loss, and the active work/stop affordance remains above the composer. The transcript follows the tail only while the user is already at the tail. | `layout-invariants.spec.ts`: long prompt fixture, collapse/expand, tail and composer-clearance geometry. |
| Composer + queue + terminal | A fixed scroll padding could not account for an expanded queue or the composer moving above the terminal. | Transcript bottom clearance derives from the measured composer dock, not an arbitrary static padding value. | Playwright long-prompt state; existing Bombadil `transcript_content_clears_composer`. |
| Working composer composition | The working-state keyboard instruction sat as a full-width line above the textarea, while a global focus rule drew a second orange rectangle inside the composer. At some scale combinations this made the empty composer look oversized and unfinished. | A working composer has one bounded input surface: its concise mode status lives in the toolbar, the textarea has a fixed comfortable height, and focus appears once on the outer shell. | `poolside-polish.spec.ts`: focused working fixture checks compact height, toolbar-contained hint, quiet textarea focus, no overflow, and absence of internal runtime jargon. |
| Model picker + terminal | The supplied CUA capture showed the picker extending into the terminal/composer stack, risking a menu that was partly obscured or unreachable. | A visible picker stays in the viewport, clears the terminal, has scrollable bounded height, and owns hit-testing at interior points. | `composer-surfaces.spec.ts` fuzzes 960×640, 1024×700, 1280×840, and 1440×900; matching Playwright geometry test. |
| Sidebar history | A large automatically titled history filled the sidebar and hid the useful navigation/residency context. | Sidebar begins compact; pinned, active, and working chats are retained; the complete history remains one reversible action away. | `sidebar-navigation.spec.ts`: 14-session fixture proves the 10-row compact state, working retention, show all, and show fewer. |
| Search dialog | A dense result list was clipped at the dialog's rounded lower edge instead of consuming the dialog's remaining height and scrolling. | The search input owns its fixed row; results use the remaining dialog height, scroll internally, keep their final row fully visible, and never widen the page. | `sidebar-navigation.spec.ts`: 24-session fixture scrolls to and opens the final result, while asserting dialog/list/result geometry and horizontal containment. |
| Local inference telemetry | A very short sample window rendered an impossible nine-digit `tok/s` value, which dominated the rail and damaged trust. | Unmeasured or implausible rate samples are `Unavailable`, never fabricated or visually promoted. | `test_inference_telemetry.py` rejects sub-10ms derived spans; `inference_panel.test.mjs` rejects nine-digit renderer values. |
| Inference request row | Three request outcome cells were forced through a two-value grid and wrapped inconsistently. | The request row declares three metric columns and preserves its compact layout. | Component markup/class regression plus visual CUA sweep. |

## Fuzz protocol

1. Start from the packaged app. Exercise landing, an active chat, the terminal,
   model/effort menus, Outputs, a visual/container split pane, Search,
   Connectors, Settings, and the inference rail.
2. Repeat each layered interaction at the supported viewport floor and common
   desktop sizes: 960×640, 1024×700, 1280×840, and 1440×900.
3. For every visible overlay, assert all four conditions: it is in bounds, it
   is painted on top where it claims to be, it has a keyboard exit, and it does
   not create document overflow.
4. For every live surface, assert chronology: latest user message → active
   activity/working state → composer. Nothing may be hidden beneath a floating
   control.
5. Add the failure to this table and a non-vacuous test before calling it
   polished. Screenshot review is evidence, not the regression lock.

## Focused commands

```bash
npx playwright test --config apps/synth_desktop/playwright.config.ts \
  apps/synth_desktop/tests/playwright/layout-invariants.spec.ts \
  apps/synth_desktop/tests/playwright/sidebar-navigation.spec.ts

BOMBADIL_TIME_LIMIT=20s npm run test:bombadil:composer-surfaces \
  --workspace @synth/synth-desktop

uv run --project services/laguna-daemon python -m unittest discover \
  -s services/laguna-daemon/tests -p 'test_inference_telemetry.py'
```

Bombadil is intentionally stateful: it opens the terminal and the real model
picker before exploring the size set. Do not replace that with a static DOM
assertion—the point is to catch layer-order failures users can actually hit.

## Next CUA lanes

- Inspect real stream output at normal, dense, and failed tool activity levels;
  do not accept unreadable monospaced thought blocks or summary/event ordering
  inversions.
- Fuzz narrow split-pane combinations (Outputs, visual, container, inference)
  and ensure every side rail has a legible compact breakpoint.
- Use a real multi-chat history with title collisions to settle the final
  title/deduplication policy; the compact sidebar is a containment fix, not a
  substitute for better automatic titles.
