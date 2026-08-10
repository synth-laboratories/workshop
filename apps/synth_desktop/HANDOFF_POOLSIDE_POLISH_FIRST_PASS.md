# Handoff: Poolside-inspired Desktop polish — first pass

**Date:** 2026-08-09  
**Audience:** Engineer taking the first implementation pass, followed by product/engineering review  
**Scope:** Tool-activity presentation, steer/enqueue, persistent layouts, conversation management, settings polish, and accessibility/interaction consistency  
**Required:** Implement the behavior and add both Playwright and Bombadil coverage that demonstrates it works across the state matrix in this document.

Use the repo-level [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](../../WORKSHOP_QUALITY_STYLE_GUIDE.md) for the shared visual, accessibility, runtime-honesty, and definition-of-done bar; this handoff supplies the Poolside-specific workstream details.

---

## 0. Outcome

Make Synth Desktop feel calm and dependable during long-running, multi-chat work. The user should be able to control how much activity is visible, safely submit while a turn is running, restore their workspace, manage conversations, configure the experience, and operate the entire surface with a keyboard or assistive technology.

This is inspired by the interaction quality observed in Poolside Assistant 1.4.0, not a request to copy its visuals or implementation.

---

## 1. Read before changing code

- [`HANDOFF_POLISH_CUA_TESTS.md`](./HANDOFF_POLISH_CUA_TESTS.md) — dogfood loop and existing design-debt conventions
- [`HANDOFF_SESSION_LIFECYCLE_E2E.md`](./HANDOFF_SESSION_LIFECYCLE_E2E.md) — session health/lifecycle work owned by another engineer
- [`polish.md`](./polish.md) — append every shipped or flagged item
- [`testing.md`](../../testing.md) — canonical suite map
- [`tests/playwright`](./tests/playwright) — deterministic browser coverage
- [`tests/bombadil/layout.spec.ts`](./tests/bombadil/layout.spec.ts) and [`tests/bombadil/run.mjs`](./tests/bombadil/run.mjs) — current generative/stateful harness

The worktree contains concurrent changes. Do not reset, clean, or bulk-stage it. Inspect before editing and preserve work that belongs to other lanes.

### Boundary with lifecycle work

Do not redesign app-server ownership, detached-session reconciliation, Laguna cancellation, or SQLite run recovery in this pass. Consume the lifecycle states exposed by that work and render them consistently. Coordinate before changing shared session/run types.

---

## 2. Product principles

1. **No fake controls.** If an action is unavailable, explain why and disable it. Do not show a successful toast for an operation the backend did not perform.
2. **One durable source per preference.** Use the existing settings/persistence boundary. Do not scatter independent `localStorage` keys or component-only defaults.
3. **Chronology is sacred.** Tool grouping, steering, queuing, chat switching, and reloads must never reorder transcript events.
4. **Compact does not mean lost.** Hidden tool detail remains inspectable and accessible.
5. **Keyboard and pointer paths are peers.** Every pointer-only operation needs a keyboard path, visible focus, and an accessible name.
6. **Test behavior, not implementation trivia.** Prefer user-observable outcomes and durable invariants over snapshots of internal state.

---

## 3. Workstream A — tool-activity presentation

Add one persisted setting with three modes:

| Mode | While the turn is running | After the turn finishes |
| --- | --- | --- |
| **Detailed** | Show every tool/command and agent progress event | Keep detail available; default expanded state may remain detailed |
| **Grouped** | Group adjacent activity by turn/run while preserving agent messages and current activity | Collapse to a concise summary with an explicit expand control |
| **Compact** | Show only the current activity plus a count/summary of prior activity | Show a single collapsed summary |

### Acceptance criteria

- Mode can be changed from Settings and, if appropriate, from the activity summary menu.
- The chosen mode applies immediately without reordering, duplicating, or deleting events.
- Tool name, state, elapsed time, failure state, and output availability remain recoverable in every mode.
- Expanding a group shows the original chronological events.
- Running, succeeded, failed, cancelled, interrupted, and unhealthy/detached states have distinct honest presentation.
- A screen reader receives a concise status announcement when activity changes; decorative animation is not repeatedly announced.
- Large tool payloads do not cause horizontal overflow or cover the composer.

---

## 4. Workstream B — steer versus enqueue

When a turn is active, support two explicit submission intents:

- **Steer:** deliver input to the active turn using the real supported runtime operation.
- **Enqueue:** retain input as the next user turn and submit it once the current turn reaches a terminal state.

Add a persisted preference deciding which action Enter performs while an agent is working. The alternate action uses Cmd+Enter. When idle, Enter submits normally and the UI should not introduce needless steer/queue language.

### Acceptance criteria

- Composer copy and affordances make the pending action clear before submission.
- Queued prompts are visible, ordered, editable/removable where feasible, and never rendered above the user message that created them.
- Multiple queued prompts preserve FIFO ordering.
- Stop/cancel does not silently discard queued prompts. The user can choose to send next, keep, edit, or remove them.
- Steer never creates a fake completed user turn if the runtime rejects it. Surface a recoverable error and keep the text.
- Switching chats does not move queued input to another chat.
- Reload/relaunch behavior is explicit and tested. Durable queued prompts should survive; intentionally ephemeral drafts must be labeled and must not masquerade as queued work.
- Double-submit, key repeat, mouse plus keyboard races, and a terminal-state transition during submission do not duplicate messages.

If the current backend lacks a real steer primitive, define the command/result contract and leave an expected-failing end-to-end test rather than simulating success in the renderer. Queueing may ship independently if it is honest.

---

## 5. Workstream C — persistent workspace layouts

Persist and restore:

- Left and right sidebar visibility and width
- Output/secondary pane visibility and width
- Bottom panel visibility and height, if present
- Selected conversation and selected output tab when those records still exist
- Per-project/session layout where the product already has a project identity
- A user-selected default layout for new workspaces

### Acceptance criteria

- Resize handles are discoverable, keyboard operable, and constrained to useful minimum/maximum sizes.
- A narrow window clamps invalid saved sizes instead of pushing content off-screen.
- Reload and full app relaunch restore the last valid layout.
- Missing/deleted tabs, conversations, projects, displays, or outputs fall back safely.
- Moving between monitors or changing display scale does not restore the window or panes off-screen.
- “Save layout as default” and “Reset layout” are available from Settings or the Window menu.
- Chat input remains visible and usable with every supported pane combination.
- Persistence is versioned or normalized so future layout fields can be added without breaking old settings.

---

## 6. Workstream D — conversation management

Provide consistent conversation actions from the sidebar and search results:

- Rename
- Pin/unpin
- Archive/unarchive
- Duplicate, only if the product can define exactly what is copied
- Delete permanently behind explicit confirmation, if deletion is retained at all

Organize active conversations with stable grouping (for example pinned, working/attention, recent, and project/date groups) without causing titles to jump as status icons change.

### Acceptance criteria

- Archive is the safe default removal action; archived chats have a dedicated Settings/page surface.
- Rename supports keyboard confirm/cancel, rejects or normalizes empty names, and preserves Unicode.
- Pin order is deterministic across reloads.
- Working and finished-unviewed indicators coexist with pin/archive state and have accessible labels.
- Opening a finished-unviewed chat clears its unread marker at the correct time, not merely on hover.
- Context menus support keyboard invocation, arrow navigation, Escape, and focus return.
- Search results update after rename/archive and never route to a missing conversation.
- Active runs are not accidentally destroyed by archive, rename, or pin. If an operation is unsafe, explain and disable it.
- Empty, loading, and failure states offer a useful recovery action.

---

## 7. Workstream E — settings polish

Reorganize Settings into clear sections and add real persisted controls for this pass:

- General/appearance: system, light, dark; translucency only if the native shell supports it honestly
- Chat font size
- Code font family and size
- Terminal font family and size, if the terminal is in scope
- Prompt submission: steer/enqueue Enter behavior
- Tool activity: Detailed/Grouped/Compact
- Layout: save current as default, apply/reset default
- Keyboard shortcuts
- Archived chats
- Models/agents: link to the existing capability/effort configuration rather than duplicating it
- About: version/build information and changelog entry point

### First-pass boundary

Do not build a fake updater, voice-model downloader, remote-access service, or app-icon switcher to match Poolside. It is acceptable to establish the settings information architecture and omit unsupported sections. Every visible setting must affect real behavior and persist.

### Acceptance criteria

- Settings are searchable or at least directly navigable with stable section anchors.
- Invalid numeric/font values are rejected or clamped with clear feedback.
- Theme/font changes preview immediately and survive reload/relaunch.
- Reset restores documented defaults.
- Settings migration handles missing, old, and malformed values.
- There is one canonical label for every setting across menus, tooltips, and tests.

---

## 8. Workstream F — accessibility and interaction consistency

Treat this as a cross-cutting requirement, not a cleanup after the other five workstreams.

### Acceptance criteria

- All controls have accessible names, roles, state, and keyboard behavior.
- Focus order follows the visible layout and never enters hidden/collapsed content.
- Dialogs and menus trap focus appropriately, close on Escape, and return focus to the invoker.
- Focus rings are visible in light, dark, translucent, selected, and disabled contexts.
- Hit targets around small icons are at least 24×24 CSS px; primary controls should target 32×32 or larger.
- Status is not conveyed by color alone. Working, unread, failed, and unhealthy states include shape/text/accessible labels.
- Reduced-motion disables nonessential pulsing/sliding while preserving state changes.
- At 200% zoom and narrow supported widths, content reflows without horizontal page overflow or composer obstruction.
- Tooltips are consistent, delayed appropriately, and include shortcuts where useful.

Run the existing accessibility/static suite and add rules for new controls and prohibited regressions.

---

## 9. Required Playwright coverage

Add focused specs rather than one enormous serial scenario. Use stable `data-testid` values only where semantic locators are insufficient.

At minimum, cover:

### Tool activity

- Each mode while running and after success, failure, cancellation, interruption, and unhealthy/detached reconciliation
- Switching modes mid-turn and after completion
- Expand/collapse restores complete chronological content
- Long names/payloads at narrow and wide viewport sizes

### Steer/enqueue

- Idle submit, active steer, active enqueue, alternate Cmd+Enter action
- FIFO with at least three queued prompts
- Edit/remove queued prompt
- Stop, runtime rejection, network failure, reload, and chat switch with queued input
- Race: run becomes terminal while submission is dispatched
- No duplicate messages from rapid or mixed pointer/keyboard submission

### Layout

- Resize/hide/show every pane and reload
- Relaunch-equivalent persistence using a fresh page/context and the real settings adapter
- Invalid/malformed saved layout normalization
- Narrow viewport, zoomed UI, and deleted selected-resource fallback
- Save default, apply to new workspace, and reset

### Conversations

- Rename, pin, archive, unarchive, search, and unread clearing
- Keyboard context-menu operation and focus restoration
- Actions on idle, working, finished-unviewed, failed, interrupted, and unhealthy conversations
- Empty archive/search states and backend failure recovery

### Settings/accessibility

- Every setting changes real UI behavior and persists in a fresh context
- Keyboard-only completion of all primary flows
- Accessible names/states and focus behavior for menus/dialogs
- Reduced-motion and 200% zoom/narrow viewport checks
- No horizontal document overflow and no composer overlap

Do not satisfy persistence tests by reusing an in-memory React store. Exercise the same adapter used by production. When a native-only path cannot run in browser Playwright, provide a contract-faithful IPC fixture and add an installed-app acceptance step.

---

## 10. Required Bombadil coverage

Extend the existing Bombadil model rather than writing a disconnected demo model. It should generate long, adversarial sequences across chats, run states, layouts, settings, and keyboard/pointer actions.

### Actions to model

- Start/finish/fail/cancel/interrupt a run
- Change activity mode and expand/collapse groups
- Submit idle, steer, enqueue, edit/remove queued input
- Switch/create/rename/pin/archive/unarchive conversations
- Mark finished-unviewed and open the conversation
- Open/close/resize panes; change viewport; reload/restore
- Open/close Settings; change/reset preferences
- Keyboard navigation, Escape, Enter, Cmd+Enter, repeated key events

### `always` invariants

- Transcript events remain in monotonic chronological order with stable identities.
- No user message or queued prompt is duplicated, silently lost, or attached to the wrong conversation.
- At most one active run is presented per conversation unless the backend contract explicitly supports more.
- The working/finished/unread/sidebar state agrees with the canonical session projection.
- Archived conversations do not appear in the active list; unarchiving restores exactly one entry.
- Selected conversation/output references either exist or have fallen back to a valid selection.
- Pane sizes remain finite and within current viewport bounds; the composer remains reachable.
- Hidden/collapsed controls cannot receive focus.
- Persisted settings always normalize to a supported schema and enum value.
- Compact/grouped presentation never destroys the underlying inspectable activity.

### `eventually` properties

- A queued prompt is either submitted after the active run terminates or remains visibly actionable; it never disappears.
- A completed unseen conversation becomes seen after the defined open/read interaction.
- Saved settings/layout converge to the same visible state after reload.
- Focus returns to a valid, visible target after menus/dialogs close.

Use a reproducible seed and preserve the failing action trace in `test-results/bombadil`. Add directed coverage for every important transition before relying on random exploration. A passing short run is not enough: configure enough actions/seeds to traverse every modeled run state and each conversation/layout operation.

---

## 11. Test matrix — interpret “under all circumstances” concretely

Every feature does not need a literal Cartesian-product test, but the suite must cover pairwise interactions and Bombadil must explore cross-feature sequences across these dimensions:

| Dimension | Required states |
| --- | --- |
| Run | idle, starting, running, succeeded, failed, cancelled, interrupted, unhealthy/detached |
| Conversation | selected, background, finished-unviewed, pinned, archived, renamed |
| Submission | normal, steer, one queued, multiple queued, rejected, raced with terminal transition |
| Window | wide, minimum supported, 200% zoom, panes hidden/open, restored invalid dimensions |
| Input | pointer, keyboard, key repeat, screen-reader semantics |
| Persistence | initial defaults, reload, fresh context/relaunch, malformed/old stored schema |
| Theme/motion | light, dark, system where deterministic, reduced motion |
| Backend | success, slow, explicit error, disconnect/reconcile projection |

Any intentionally unsupported intersection must produce a clear disabled state or recoverable error and have a test proving that behavior.

---

## 12. Suggested implementation order

1. Inventory existing settings, session projection, composer commands, and layout persistence; document the chosen ownership boundary.
2. Add/normalize typed settings schema and migration before wiring controls.
3. Implement tool-activity modes.
4. Implement honest enqueue, then real steer if the backend supports it.
5. Implement layout persistence/default/reset.
6. Implement conversation actions and archived surface.
7. Finish Settings IA and accessibility consistency across all new surfaces.
8. Add Playwright coverage alongside each slice, then extend Bombadil actions/invariants.
9. CUA the installed app, including a full relaunch, narrow window, keyboard-only pass, and at least two simultaneous chats.
10. Append the shipped/flagged/tested result to [`polish.md`](./polish.md).

---

## 13. Commands and evidence

Use the repository’s canonical commands; verify current scripts before running:

```bash
npm --prefix apps/synth_desktop run typecheck
npm run test:a11y --workspace @synth/synth-desktop
npx playwright test --config apps/synth_desktop/playwright.config.ts
npm run test:bombadil --workspace @synth/synth-desktop
npm run desktop:verify
```

In the review handoff, report:

- Exact features shipped and intentionally deferred
- Architecture/persistence decisions
- Exact Playwright specs and cases added
- Bombadil actions, invariants, seeds/action count, and any minimized failing traces
- Commands and exact pass/fail counts
- Installed-app CUA evidence, including reload/relaunch and keyboard-only results
- Screenshots or short recordings for major visual states
- Known unsupported intersections from the test matrix

---

## 14. Definition of done for the first pass

- [ ] All six scoped workstreams have a real implementation or an explicit, tested deferral
- [ ] No fake steer, persistence, archive, updater, or accessibility behavior
- [ ] Playwright covers the directed state matrix and failure/restart cases
- [ ] Bombadil explores cross-feature sequences and enforces the listed invariants
- [ ] Typecheck, accessibility/static tests, focused Playwright, and Bombadil are green
- [ ] Installed Desktop passes CUA reload/relaunch, narrow-window, keyboard-only, and multi-chat acceptance
- [ ] New controls use stable semantics/test hooks and do not regress existing lifecycle behavior
- [ ] `polish.md` is appended
- [ ] Changes are committed in reviewable slices without staging unrelated work
