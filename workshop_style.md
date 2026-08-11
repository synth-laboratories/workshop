# Provisional Workshop style and quality triage

**Status:** provisional working guidance  
**Related:** [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](./WORKSHOP_QUALITY_STYLE_GUIDE.md)

This document is the short debugging version of the Workshop quality bar. It answers a practical question: what is categorically unacceptable, what must be fixed before review, and what may be flagged until a product or backend contract exists?

## The standard

Workshop should feel calm, precise, inspectable, and trustworthy. “Feels like Linear” means clear hierarchy, stable layout, excellent keyboard behavior, and honest states—not copying another product’s visuals.

The unacceptable pattern is shipping something that looks finished while being dishonest, unreachable, fragile, or unusable.

## Ship blockers: fix immediately

### 1. Trust violations

The UI must never claim that something happened when it did not.

Unacceptable examples:

- Showing `Working` or `Stop` after the agent process is gone
- Showing `Completed` when the backend rejected the action
- Fake download or progress percentages
- Exposing model effort options the provider does not support
- Claiming a queued or steered message was accepted when it was not
- Showing raw secrets, API keys, or sensitive tool payloads

### 2. Data and lifecycle corruption

The user’s work must not be lost, reordered, duplicated, or attached to the wrong conversation.

Unacceptable examples:

- Assistant text appearing before the user message that caused it
- Queued prompts disappearing on stop, reload, or relaunch
- A process restart creating a duplicate session
- An old process detaching a newer process
- Events from one chat appearing in another
- Stop throwing because the process already exited

### 3. Security and privacy failures

Secrets and private data must stay inside the trusted boundary.

Unacceptable examples:

- API keys in transcripts, terminal output, traces, logs, screenshots, or fixtures
- Arbitrary paths or commands crossing the workspace boundary
- Renderer-only state pretending to be durable runtime authority

## Fix before review

### 4. Dead or deceptive controls

Every visible control must work, explain why it cannot work, or be omitted.

Examples:

- Connectors or Search buttons that do nothing
- Titlebar actions that only show a stub toast
- “Set up an agent” with no real flow
- Visible Downloads or Expand actions with no behavior
- Enabled buttons that cannot succeed

If the intended behavior is clear, implement it. If the backend/product contract does not exist, remove or disable the control and add an expected-failing test.

### 5. Layout failures

The interface must remain usable at supported sizes and pane combinations.

Examples:

- Text overflowing below or behind the composer
- Activity and Outputs controls overlapping
- Composer hidden by a pane or terminal
- Horizontal scrolling from long IDs or tool output
- Clipped labels or buttons
- Restored pane sizes pushing content off-screen
- Huge blank areas caused by unstyled or orphaned controls

### 6. Accessibility and input failures

Keyboard and assistive-technology behavior are product behavior.

Examples:

- No visible focus
- Icon-only buttons without accessible names
- Menus that cannot be navigated or dismissed with the keyboard
- Pointer-only actions
- Status communicated only by color
- Hit targets too small
- Motion that ignores reduced-motion preferences

### 7. Broken chronology or activity presentation

Transcript chronology is sacred. Presentation modes may hide detail, but may not change the underlying order.

Check:

- User → tool activity → assistant output order
- Running, completed, failed, cancelled, interrupted, detached, unhealthy, and queued states
- Detailed, Grouped, and Compact modes preserving recoverable detail
- Secrets and large payloads redacted without destroying useful context

## Fix when touching the surface; otherwise log as polish debt

### 8. Weak visual hierarchy

Examples:

- Technical IDs presented as primary titles
- Every control having equal visual weight
- Metadata more prominent than the result
- Sparse empty states without a next action
- Dense walls of repeated chat names
- Internal implementation or parity language shown to users
- Inconsistent buttons, status chips, spacing, or typography

### 9. Content and state copy problems

Copy should say what happened and what the user can do next.

Prefer:

- `Connecting to Laguna…`
- `No optimizer runs yet. Import a local run or create a cloud run.`
- `Couldn’t connect to Craftax at 127.0.0.1:8098. Check the service, then retry.`
- `This session lost its agent process. The transcript is safe; send another message to reconnect.`

Avoid:

- `Done` when a command only started
- `Working…` after process loss
- Raw exception text without recovery guidance
- `Stub`, `TODO`, or `Not implemented` in a ship build
- Claims about provider/model capabilities that are not true

## Flag with an expected-failing test

Flag instead of pretending to fix when:

- The runtime lacks a real primitive, such as steer
- Product semantics are undefined, such as duplicate or permanent-delete behavior
- A backend/provider contract is missing
- A native service is required but unavailable in the current test environment
- A migration or cross-runtime boundary is not yet implemented

Every flag needs:

1. A user-observable expected-failing Playwright test, when practical.
2. A static grep/design-debt lock when the issue is a forbidden stub or residue.
3. A short entry in [`apps/synth_desktop/polish.md`](./apps/synth_desktop/polish.md).
4. A named owner or dependency if the issue is not self-contained.

Do not use a flag as an excuse for a visual defect that can be fixed locally.

## Architecture smells to fix or flag

- Renderer invents durable session/run/lifecycle state.
- Multiple registries describe the same model, visual, or connector concept.
- Preferences are scattered across component-local storage keys.
- A new control has no stable behavioral test.
- A change is only verified in a browser fixture when it affects the native shell or process lifecycle.
- CUA was not run for installed-app shell, drag, terminal, native bridge, or lifecycle changes.

## Debugging decision tree

1. **Does it lie, lose data, expose secrets, or corrupt chronology?** Fix immediately.
2. **Does it block a normal user path?** Fix before review.
3. **Does it cause overflow, inaccessible interaction, or broken responsive layout?** Fix before review.
4. **Is the intended behavior clear but missing?** Implement it.
5. **Is behavior undefined or dependent on a missing contract?** Remove/disable it, add an expected-failing test, and document the dependency.
6. **Is it inconsistent or visually weak but non-blocking?** Fix when touching that surface; otherwise log it as polish debt.

## Minimum proof before calling a fix complete

- CUA on the real Desktop path when native shell or runtime state is involved
- Playwright for the user-observable behavior
- Bombadil for geometry/state invariants that vary across viewports
- Static/a11y coverage for test IDs, forbidden stubs, bridge contracts, and accessibility residue
- Typecheck and relevant Rust tests
- Checks at 960×640, 1280×840, and 1440×900 where layout is involved
- `polish.md` entry describing shipped behavior, tests, flags, and CUA notes

## One-line rule

**Never ship a polished-looking lie; fix it, remove it, or flag it with a test.**

