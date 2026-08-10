# Workshop quality and style guide

**Status:** living product guidance  
**Audience:** product, design, frontend, Rust/runtime, and QA engineers  
**Applies to:** Synth Desktop and its visual/artifact surfaces; not `apps/mock`  
**Last reviewed:** 2026-08-09

This is the shared bar for Workshop. It turns the principles scattered across the CUA handoff, Poolside polish handoff, `testing.md`, visual tokens, and `polish.md` into one reviewable standard.

## 1. Product character

Workshop is a local-first agent research and development workbench. It should feel calm while work is running, precise when work is inspected, and trustworthy when something is unavailable.

The product personality is:

- **Quietly capable.** The interface stays out of the way while an agent works.
- **Inspectable.** Runs, tool activity, traces, visuals, costs, and state transitions can be opened and understood.
- **Honest.** A missing capability is named clearly; the UI never claims an action succeeded when the runtime did not perform it.
- **Dense, not cluttered.** Information is close enough to scan, with hierarchy and whitespace doing the organizing.
- **Warm but technical.** Synth orange and the logo provide identity; system typography, restrained surfaces, and purposeful monospace provide the workbench feel.

“Feels like Linear” means clarity, hierarchy, keyboard quality, stable layout, and excellent empty/error states. It does not mean copying Linear or Poolside pixel-for-pixel.

## 2. Source of truth

Use the existing architecture before introducing a new pattern.

| Concern | Canonical source |
| --- | --- |
| Desktop colors, type, spacing, radii, shell geometry | `apps/synth_desktop/src/renderer/src/styles/app.css` |
| Artifact/visual chrome tokens | `visuals/chrome/tokens.css` |
| Model-specific controls and effort knobs | `apps/synth_desktop/src/renderer/src/runtime/modelCapabilities.ts` |
| Durable preferences and layout normalization | `apps/synth_desktop/src/renderer/src/preferences/` |
| Sessions, runs, events, inventory, visuals, and lifecycle state | Rust CoreRuntime + SQLite in `apps/synth_desktop/src-tauri` |
| CUA findings and shipped/flagged polish | `apps/synth_desktop/polish.md` |
| Test commands and coverage boundaries | `testing.md` |

Do not create a second token system, a component-local preference store, or a renderer-only source of runtime truth.

## 3. Visual foundations

### 3.1 Color

Desktop uses a light, neutral canvas with a single warm accent. New UI should consume variables rather than introduce one-off hex values.

| Role | Token / intent |
| --- | --- |
| Main canvas | `--color-bg` |
| Subtle canvas / selected navigation | `--color-bg-subtle` |
| Sidebar and shell chrome | `--color-sidebar`, `--color-tab-bar` |
| Dividers | `--color-border` |
| Primary text | `--color-text` |
| Supporting text | `--color-text-muted` |
| Tertiary metadata | `--color-text-faint` |
| Synth action / focus accent | `--color-accent` / `--color-accent-ring` |
| User message emphasis | `--color-blue` |
| Logo identity | `--color-logo` |

Rules:

- Use accent for action, selection, progress, and important visual identity—not every link or decoration.
- Do not use color alone to communicate status. Pair it with text, an icon, or an accessible label.
- Keep destructive/error colors reserved for destructive/error states.
- Dark theme values must preserve the same semantic roles and readable contrast; never merely invert the light palette.
- Artifact surfaces may use `--sv-*` tokens, but should still feel like the same product.

### 3.2 Typography

- Use the system sans stack for product copy and controls.
- Use the configured monospace stack only for paths, IDs, commands, code, cursors, and machine-generated values.
- Use sentence case. Reserve uppercase/small caps for short section eyebrows and status labels.
- Establish hierarchy with size, weight, and spacing before adding color.
- Body copy should remain comfortably readable at the default chat size. Metadata may be smaller, but must not become the primary way to understand a record.
- Keep long titles and IDs bounded with ellipsis or wrapping. Never allow machine text to push a control off-screen.
- Use tabular numerals for counts, durations, tokens, and costs.

Recommended hierarchy:

| Element | Guidance |
| --- | --- |
| Page title | 20–24px, semibold, tight tracking |
| Section title | 15–18px, semibold |
| Body / transcript | 14px default, 1.45–1.6 line-height |
| Supporting copy | 12–13px, muted |
| Labels / metadata | 10–12px, only when supporting a visible primary value |
| IDs / paths | 11–12px monospace, wrap or ellipsize |

### 3.3 Shape, spacing, and depth

- Build on the existing 8px rhythm. Prefer 4, 8, 12, 16, 20, and 24px spacing values.
- Use `--radius-sm`, `--radius-md`, and `--radius-lg`; do not introduce a new radius for a single card.
- Borders should establish grouping. Shadows should establish elevation or a floating surface, not decorate every card.
- Prefer one clear container over nested cards inside cards.
- Selected states should be visible but quiet: subtle surface tint, border, or inset accent is usually enough.
- Avoid giant empty regions caused by unstyled controls, orphaned labels, or an inspector that has no useful content.

## 4. Shell and layout

The shell is a workbench, not a marketing page.

- Sidebar, titlebar, tabs, main workbench, composer, output pane, and terminal have stable regions and clear ownership.
- Titlebar actions stay compact and never compete with the active tab or runtime status.
- Keep generous drag regions, but interactive controls must remain non-draggable and clickable.
- A fixed composer must remain fully visible and usable with every supported pane combination.
- Transcript content must end above the composer with a deliberate reading gap; do not rely on a fixed magic height when the composer can resize.
- Chat + visual/container panes should have a clear divider and predictable collapse behavior.
- Outputs, Activity, and other floating controls must share a toolbar or collision-proof layout. No control may cover another at any supported viewport.
- Treat 960×640, 1280×840, and 1440×900 as required desktop states. Also inspect the compact breakpoints used by the app.
- Clamp restored widths/heights to useful bounds. A saved layout must never push the composer, titlebar, or pane off-screen.
- Use scrolling inside the owner region, not a page-wide overflow that hides the composer or titlebar.

### Responsive behavior

At narrower widths, reduce columns before reducing legibility:

1. Collapse secondary panes into a stacked or explicit open state.
2. Allow toolbars to wrap into intentional rows.
3. Preserve button labels for primary actions; shorten only secondary metadata.
4. Keep a usable composer and visible focus target.
5. Test for horizontal overflow after every layout change.

## 5. Component standards

### Navigation and conversations

- Primary destinations are obvious: New conversation, Connectors, Search, Chats, Cloud, Research, Inventory, and Settings.
- Chat rows need a stable title, target/model context where useful, and distinct working / finished-unviewed / idle states.
- A working indicator should animate only while work is active. A finished-unviewed indicator persists until the user opens the chat.
- Pin, archive, rename, and unread state must coexist without making rows jump unpredictably.
- Automatically generated titles should be short, human-readable, and stable. Technical prompt fragments are not a substitute for a title.
- Empty sections should explain what belongs there and provide a real next action when one exists.

### Titlebar and tabs

- Keep titlebar status short; detailed diagnostics belong in a tooltip or a dedicated surface.
- Every visible titlebar control must either work, be clearly disabled with an explanation, or be omitted until it works.
- Tabs use one active treatment, one close affordance, and a predictable New tab action.
- Never place a toast or dead-end stub where navigation or a real settings destination is available.

### Buttons and controls

- Every action has a clear verb: `Open visual`, `Refresh`, `Import trace`, `Save access`, `Send next`.
- Primary action: one per region when possible. Secondary actions use quiet bordered controls.
- Destructive actions require confirmation and name the consequence.
- Disabled controls explain why through visible copy or an accessible description.
- Maintain a minimum 32px hit target for new controls; larger is preferable for primary actions.
- Focus is always visible. Keyboard and pointer paths are peers.
- Menus support Escape, arrow navigation, selection feedback, and focus return.

### Chat and activity

- Preserve chronology. User input, tool activity, assistant output, interruptions, and errors must render in event order.
- Detailed, Grouped, and Compact are presentation modes, not different data models. Hidden detail remains recoverable.
- Running, completed, failed, cancelled, interrupted, detached, unhealthy, and queued states must not collapse into one generic “Working” label.
- A Stop action must reconcile an already-gone process instead of surfacing an opaque exception.
- Steer is only offered when a real runtime primitive exists. If unsupported, keep the draft and explain the honest fallback.
- Queued input is durable only when it is actually durable; drafts must not masquerade as queued turns.
- Tool rows show the tool identity, state, elapsed time where available, and whether output is available. Secrets and raw payloads are redacted.
- Avoid animation that competes with the message being read. Respect reduced-motion preferences.

### Composer

- The composer is always the most obvious place to type. Placeholder copy states the current target without pretending a disabled runtime is ready.
- Model, effort/thinking, permissions, voice, and send controls have stable order and alignment.
- Model-specific knobs come from the capability registry; do not branch on model IDs throughout the renderer.
- Use the full model identity where it prevents ambiguity. Use the actual provider contract for effort options—do not expose unsupported levels.
- Enter/Cmd+Enter behavior is explicit while a turn is active and ordinary when idle.
- Multi-line input, long pasted text, queued prompts, and validation errors must not overflow below or behind the composer.

### Settings

- Settings are organized by user intent: General, Models, Runtime, Account, and About.
- Every visible preference changes real behavior and persists through reload/relaunch.
- Invalid values are rejected or normalized with immediate, specific feedback.
- Keep internal implementation notes, migration caveats, and “unsupported parity” language out of normal user-facing copy.
- If a Poolside feature is not supported, omit it or describe the available Workshop alternative; do not ship a fake updater, downloader, remote-access switch, or icon selector.

### Inventory, traces, and visuals

- Inventory records need a clear identity, status, primary metadata, and the next useful action.
- Empty Containers/Traces/Usage states should name the attach/import path and default endpoint where applicable.
- Trace and rollout views should make chronology, metrics, actions, rewards, and evidence inspectable together.
- A visual has one durable `visual_id` across chat, registry, and preview pane.
- “Open visual,” “Go to chat,” and “Back” are real navigation actions, not decorative text.
- Use the strongest existing trace catalog surface as the density reference: summary, search/filter, status, and structured rows.

### Terminal and monitors

- The terminal is a distinct tool surface with clear tabs, running/exited state, focus, and readable contrast.
- Terminal output uses the configured terminal font and supports selection without making the surrounding workbench look broken.
- Inference monitors are secondary diagnostics: useful when open, quiet in the titlebar, and explicit when no samples or weights are available.

## 6. Content and state honesty

Use copy that tells the user what happened and what they can do next.

| State | Good pattern |
| --- | --- |
| Loading | “Connecting to Laguna…” / “Loading model…” |
| Ready | “Laguna XS 2.1 ready” |
| Disabled | “Unavailable — configure an API key in Account” |
| Empty | “No optimizer runs yet. Import a local run or create a cloud run.” |
| Failed | “Couldn’t connect to Craftax at `127.0.0.1:8098`. Check the service, then retry.” |
| Detached | “This session lost its agent process. The transcript is safe; send another message to reconnect.” |
| Queued | “Queued next · 2 prompts” |

Avoid:

- “Done” when a command only started.
- “Working…” after the process is gone.
- Fake percentage progress for unknown work.
- Toasts that say “stub,” “TODO,” or “not implemented” in a ship build.
- Raw exception text without a recovery action.
- Claims that imply provider/model capabilities the API does not support.

## 7. Accessibility and interaction quality

Accessibility is part of visual quality, not a separate pass.

- Prefer semantic headings, buttons, links, lists, dialogs, tabs, comboboxes, and status regions.
- Every icon-only control has an accessible name and tooltip where useful.
- Do not communicate state by color alone; include text or an accessible state.
- Maintain visible `:focus-visible` treatment and logical keyboard order.
- Dialogs and menus trap/return focus correctly and close with Escape.
- Live regions announce meaningful state changes once; decorative spinners are hidden from assistive technology.
- Respect reduced motion and avoid layout-shifting animation.
- Preserve text selection in transcripts, code, IDs, and terminal output.
- Check contrast for muted text, disabled controls, selected rows, status chips, and dark terminal surfaces.
- Verify narrow layouts, zoom, keyboard-only operation, and screen-reader names for every new surface.

## 8. Runtime and data boundaries

UI polish cannot make a false runtime contract acceptable.

- Rust CoreRuntime/SQLite owns durable sessions, runs, events, approvals, inventory, visuals, and lifecycle state.
- The renderer renders projections and sends typed commands; it does not invent durable state.
- On restart, stale `running` records are reconciled to an honest terminal/interrupted/unhealthy state.
- Stop is idempotent when the process is already gone.
- Attachment generations are fenced so an old process cannot detach a replacement.
- Provider/model capability registration is declarative and validated at the boundary.
- Secrets never render in tool arguments, terminal output, traces, logs, or screenshots.
- New persistence is versioned, normalized, and migration-tested.

## 9. Testing and dogfood gates

Every UI change should have the smallest useful test at each relevant layer.

### Required loop

1. Run the installed or canonical desktop build.
2. Use CUA on the real WebView, including the happy path and a failure/empty state.
3. Add or update a focused Playwright behavior test.
4. Add a Bombadil invariant for geometry/state that can regress across viewports.
5. Add a static/a11y lock when the issue is a stub, missing testid, bridge surface, or forbidden residue.
6. Run typecheck, relevant Rust tests, frontend build, and the focused suite.
7. Run the packaged install/acceptance path for shell, drag, terminal, native bridge, or lifecycle work.
8. Append the result to `apps/synth_desktop/polish.md`.

### Test what users observe

Prefer assertions such as:

- composer stays visible and usable;
- no horizontal overflow;
- controls do not overlap;
- an Outputs panel agrees with `aria-expanded` and clears the composer;
- working/unread indicators transition and persist correctly;
- a visual ID resolves to the same chat card, registry row, and pane;
- a process loss removes stale Working/Stop state and preserves the transcript;
- malformed preferences normalize to documented defaults;
- unsupported operations fail honestly and preserve the user's draft.

Do not make snapshots or internal component structure the only proof of quality.

### Viewport/state matrix

At minimum, exercise:

- 960×640, 1280×840, and 1440×900;
- landing, idle chat, active turn, completed-unviewed chat, error, and empty states;
- sidebar only, output pane, container pane, terminal open, and combined panes;
- light, dark, and reduced-motion preferences where supported;
- mouse, keyboard, reload, and full process relaunch for durable behavior.

## 10. CUA review checklist

Before calling a surface polished, inspect it visually and interactively:

- Is the primary action obvious within two seconds?
- Does the hierarchy read correctly at a glance?
- Are headings, labels, metadata, and IDs proportioned by importance?
- Do controls align on a shared baseline and stay within their owner region?
- Is there any clipped text, unexpected wrap, overlap, orphaned label, or giant dead zone?
- Does the empty/loading/error state tell the user what to do next?
- Does the surface remain usable with a narrow window, long title, long ID, or open secondary pane?
- Does the product make an honest claim about runtime/provider/model state?
- Can every visible action be completed with a keyboard and understood by a screen reader?
- Would the screenshot still look intentional if all data were replaced by fixtures?

If the answer is no, fix it or add an expected-failing test and log it.

## 11. Review checklist for engineers

### Before opening review

- [ ] Reused existing tokens and component patterns.
- [ ] Added stable test IDs only for meaningful user-observable controls.
- [ ] Verified copy, loading, empty, error, disabled, and success states.
- [ ] Checked long titles, IDs, tool output, and narrow widths.
- [ ] Confirmed keyboard focus, Escape, menu navigation, and accessible names.
- [ ] Confirmed no fake behavior, secret leakage, or unsupported model knob.
- [ ] Added Playwright/Bombadil/static coverage appropriate to the change.
- [ ] Ran typecheck, focused tests, and the relevant packaged/CUA path.
- [ ] Appended `polish.md` with shipped, tested, flagged, CUA, and reference notes.

### Reviewer questions

1. What is the user-visible outcome?
2. What is the source of truth for this state or preference?
3. What happens after reload, relaunch, process loss, or a late event?
4. What does the user see when the capability is unavailable?
5. Which viewport/state matrix proves the layout is safe?
6. Is this a reusable pattern or a one-off exception? If reusable, where is it registered?

## 12. Definition of done

A Workshop slice is done when:

- the real Desktop path has been CUA-reviewed;
- the surface is visually coherent with the existing Synth language;
- the runtime claim is true and recoverable;
- the layout is safe at supported viewports and pane combinations;
- keyboard and accessibility behavior are complete;
- focused Playwright/Bombadil/static tests pass;
- relevant Rust/bridge tests pass;
- `polish.md` records the result and any intentional debt.

When a feature is not ready, omit it or leave a visible, tested, honest boundary. Do not ship a polished-looking lie.

