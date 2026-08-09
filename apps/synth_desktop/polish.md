# Desktop polish log

Append-only log for UX fixes, stub removals, CUA findings, and test flips.  
Owner handoff: [`HANDOFF_POLISH_CUA_TESTS.md`](./HANDOFF_POLISH_CUA_TESTS.md).

**How to append** (every session that ships or flags something):

```markdown
### YYYY-MM-DD — <short title>
- **Shipped:** …
- **Tests:** …
- **Flagged:** …
- **CUA notes:** …
- **Refs:** …
```

---

## Sessions

### 2026-08-09 — Bootstrap (prior work)

- **Shipped:** Removed stub LoRA / Finetunes UI (Composer + Settings); Settings shows Adapters · Not wired; Inventory Attach defaults to Craftax `http://127.0.0.1:8098`.
- **Tests:** Added `tests/playwright/design-debt.spec.ts` (4 design locks + 9 `test.fail` debt flags) and `tests/design_debt.test.mjs` (static stub greps + LoRA regression locks). Documented in `testing.md`.
- **Flagged:** Account / Downloads / Expand toast stubs; Always-ask inert; Set up agent stub; Reload Laguna stub; async leave-safe `!isSync`; Codex `adapter: null`; VisualHost Craftax preview heuristics; Attach/Open-trace browser dogfood fragility.
- **CUA notes:** Empty Inventory screenshot archived at `refs/inventory-containers-empty.png`.
- **Refs:** `local_lora.md`, `containers.md`, `HANDOFF_CONTAINERS_CRAFTAX.md`, `HANDOFF_TRACES_V5.md`, Poolside Laguna S 2.1 trajectories UX.

### 2026-08-09 — Account deep-link replaces titlebar stub

- **Shipped:** The titlebar avatar now opens Settings directly on Account / backend configuration instead of leaving the current surface in place and showing `Account — stub`. Playwright renderer workers now reserve isolated loopback ports instead of sharing hard-coded `:1420`.
- **Tests:** Flipped the Account design-debt Playwright case to passing, inverted the matching static debt grep, and added a directed Bombadil Account click plus invariants for the backend-settings destination and absence of stub copy. Also flipped the concurrently completed approval-policy control from stale debt to a behavioral Playwright lock (selection + persistence) and inverted its static assertion.
- **Flagged:** Downloads, Account menu, and Expand remain stub controls; cloud Activity hierarchy and timestamp density still trail the Poolside reference.
- **CUA notes:** Reproduced the stub from a real packaged Synth Desktop cloud run; Poolside reference uses compact titlebar affordances and keeps actions tied to visible surfaces. Full-suite repetition exposed the shared-port renderer teardown as `ERR_CONNECTION_REFUSED`.
- **Refs:** `HANDOFF_POLISH_CUA_TESTS.md`, `HANDOFF_ISOLATED_DEV_INSTANCES.md`, Poolside Desktop Assistant.

### 2026-08-09 — Full Luna name and reasoning effort

- **Shipped:** Composer model chrome now uses the full `GPT 5.6 Luna` name and its Low/Medium/High/XHigh/Max effort catalog. Laguna XS 2.1 and Laguna S 2.1 use their actual binary thinking contract: Off (`none`) and On (`max`). Both preferences persist independently.
- **Tests:** Added Playwright coverage for the full Luna label and five effort choices, both Laguna targets' binary thinking payloads, persistence, native `turn/start` forwarding, and local Responses-to-MLX `enable_thinking` translation; added static accessibility locks and Rust validation coverage.
- **Flagged:** OpenRouter currently advertises optional, default-on reasoning for Laguna S 2.1 without a graded `supported_efforts` catalog, so its UI intentionally remains binary.
- **CUA notes:** Matched the supplied Poolside references: full model identity and a muted inline effort value immediately beside the model control.
- **Refs:** User screenshots, installed Codex model catalog, app-server `TurnStartParams.effort` schema, Poolside Laguna XS 2.1 model card, OpenRouter `/api/v1/models` capability metadata and reasoning guide.

### 2026-08-09 — Declarative model knob registry

- **Shipped:** Centralized model-specific composer controls into one capability registry covering target matching, labels, options, defaults, validation, per-model persistence, legacy-key migration, and `turn/start` transport. Composer and App no longer branch on Luna or Laguna model ids for knobs.
- **Tests:** Added an architectural static lock against model-specific knob branches while retaining Playwright coverage for Luna, Laguna XS, and Laguna S behavior.
- **Flagged:** Non-effort transport fields will need a bridge serializer when the first provider-specific knob uses one; the registry already makes that transport field explicit.
- **CUA notes:** No visual change; this is registration-path hardening for future model additions.
- **Refs:** `README.md` → Registering model controls; `runtime/modelCapabilities.ts`.

### 2026-08-09 — Quiet titlebar runtime status

- **Shipped:** Replaced the dense `Laguna·MLX · OR · Intern · containers/traces/visuals` capsule with a borderless status dot and a short Local ready/starting/offline label. Full backend and inventory diagnostics remain available in the native tooltip.
- **Tests:** Added Playwright and Bombadil width/content invariants across supported viewport sizes, plus a native-Laguna ready-state assertion.
- **Flagged:** None.
- **CUA notes:** Live titlebar inspection showed four unrelated concerns compressed into a clipped 180px badge beside Account; the sidebar already owns detailed model residency.
- **Refs:** User screenshot; live Synth Desktop accessibility tree.

### 2026-08-09 — Transcript clears the composer

- **Shipped:** Chat transcript bottom spacing now follows the rendered composer position instead of assuming a fixed 120px input height, preserving a 16px reading gap when the window, composer, terminal, or split view changes.
- **Tests:** Added a Playwright geometry assertion for the final message/composer boundary and a Bombadil invariant for effective transcript clearance.
- **Flagged:** None.
- **CUA notes:** The supplied desktop capture reproduced a roughly 6px overlap: the final response continued below the composer's top border.
- **Refs:** User screenshot, Poolside fixed-composer transcript behavior.

---

## Backlog seeds (optional next polish)

Pick from debt flags or CUA; log when done.

1. Wire titlebar **Account** → Settings → Account (`backend-settings`).
2. **Downloads** page or remove the control until real.
3. **Expand** real behavior or remove.
4. Composer **Always ask** → permission menu or Settings jump.
5. **Set up agent** → real flow or hide quick card.
6. Async **leave-safe** from projection, not `kind === async`.
7. Empty-state copy polish: Containers / Traces (attach/import CTAs).
8. Open Trace → guarantee PostTrain pane (flip design-debt test).
9. Trajectory inspectability vs [trajectories.poolside.ai](https://trajectories.poolside.ai) (scrub, metrics beside steps).
### 2026-08-09 — Connectors and conversation search

- CUA comparison against Poolside/Laguna confirmed that Connectors opens an MCP catalog and Search opens a focused conversation picker with ⌘K.
- Replaced both inert sidebar actions with those first-class surfaces.
- Added a data-driven connector registry so future MCP integrations are registered without duplicating page markup.
- Added Playwright flow coverage and Bombadil reachability properties for both sidebar destinations.

### 2026-08-09 — Easier window dragging

- **Shipped:** Added explicit Tauri drag regions across the sidebar strip, full titlebar, empty tab rail, and active tab label.
- **Tests:** Added a Playwright lock proving drag surfaces remain draggable while close/account controls remain clickable.
- **Flagged:** None.

### 2026-08-09 — Chat activity and unread completion

- **Shipped:** Running chats show a Codex-style animated ring; chats that finish off-screen show an orange unread dot until opened.
- **Tests:** Added a Playwright lifecycle test for running → finished/unviewed → viewed, including persistence.
- **Flagged:** None.

### 2026-08-09 — Native window dragging permission

- **Shipped:** Authorized Tauri's `start_dragging` command for the main window; renderer drag regions now work in the installed app instead of only satisfying CSS checks.
- **Tests:** Canonical install/acceptance suite plus native CUA drag after restart.
- **Flagged:** None.

### 2026-08-09 — Preserve turn order without turn-start events

- **Shipped:** A user message now closes the prior assistant draft and anchors subsequent activity/assistant output to the new turn even when the provider delays or omits `turn/started`.
- **Tests:** Extended the native two-turn Playwright stream test with activity and deltas arriving without a second turn-start event; asserts user → tool activity → assistant DOM order.
- **Flagged:** None.

### 2026-08-09 — CUA launch backpressure for real registry visuals

- **Shipped:** Added no product behavior; this is an audit-only test slice against the installed Synth Desktop app and its persisted registry state.
- **Tests:** Added Playwright `test.fail` cases for the titlebar Account-menu toast, Async Intern Respond stub, and the CUA-observed `analysis.visual.v1` payload. Added the static detector and `test:bombadil:launch-debt`, which directs Bombadil through the same registry visual.
- **Flagged:** Real CUA opened `Laguna Prompt Trim Preinstall` and hit `Visual failed to render: undefined is not an object (evaluating 's.points.map')`. The persisted agent payload uses `type: metrics` / `type: note`; the shell dispatches only `block.kind` and falls through to Scatter. Account menu, Downloads, Expand, Set up agent, Reload Laguna, and Intern Respond remain user-facing stubs.
- **CUA notes:** Inspected the real `/Applications/Synth Desktop.app` with Laguna XS 2.1 resident; Visuals listed 38 registry records and auto-previewed the broken analysis visual.
- **Refs:** `HANDOFF_POLISH_CUA_TESTS.md`; `/Users/joshuapurtell/Library/Application Support/Synth Desktop/synth.sqlite3` visual record `laguna-prompt-trim-preinstall`.

### 2026-08-09 — Remove deferred adapter residue

- **Shipped:** Removed the dormant LoRA fixture catalog, selector state, adapter-placeholder tile, and LoRA-only styling. The desktop now presents only capabilities users can actually select.
- **Tests:** Updated the Playwright and static design locks to forbid deferred-adapter UI, fixture data, and styling rather than treating `adapter: null` as product debt.
- **Flagged:** Adapter/LoRA support is intentionally deferred beyond v0.1; `local_lora.md` defines the inventory → reload → persistence → UI → installed-Tauri acceptance sequence required to reintroduce it.
- **CUA notes:** This removes the “Adapters · Not wired” promise from the real Laguna Settings surface.
- **Refs:** `local_lora.md`.

### 2026-08-09 — Typed Laguna reload

- **Shipped:** Settings → Models → Reload now invokes `window.synthLaguna.reload`, which calls the `laguna_reload` Tauri command. The manager restarts only a Synth-managed sidecar, leaves external upstreams untouched, then reprobes and returns the refreshed residency status.
- **Tests:** Flipped the Reload Laguna Playwright debt case to a passing bridge/result assertion; added static checks for the renderer and Tauri command. Typecheck, static surface tests, and `cargo check` pass.
- **Flagged:** An external upstream cannot be force-restarted by Desktop; Reload performs a fresh health probe in that configuration and reports its result honestly.
- **CUA notes:** The button now disables with Reloading… and reports ready/error text rather than silently emitting a toast.
- **Refs:** `src-tauri/src/laguna.rs`, `src-tauri/src/lib.rs`, `runtime/desktopBridge.ts`.

### 2026-08-09 — Honest scheduled model-memory release

- **Shipped:** The Residency flyout now says exactly when Laguna is scheduled to free model memory, for example `Frees at 2:15 PM · in 4m 20s`. Once that time passes while memory remains resident, it says `Free scheduled for 2:15 PM · awaiting unload` instead of falsely claiming memory is already being freed.
- **Tests:** Added Playwright coverage for both the scheduled countdown and the elapsed-but-not-yet-unloaded state.
- **Flagged:** The UI can only report the daemon's `freeAt` schedule; an actual unload completes when Laguna updates its residency status.
- **CUA notes:** The supplied desktop capture showed `Lifecycle Freeing memory…` while 20.1 GB was still resident, motivating the distinction.
- **Refs:** `src/renderer/src/components/LocalModelResidency.tsx`, `tests/playwright/runtime-regressions.spec.ts`.

### 2026-08-09 — Reconcile detached local agent sessions

- **Shipped:** Conversation, turn, and runtime-attachment state are now separate. Desktop startup reconciles orphaned running turns to interrupted; Stop is idempotent when the process is already gone; unexpected app-server exit emits a health event, persists the interrupted run, and removes only its fenced attachment generation. Sending another message lazily resumes the durable thread.
- **Tests:** Added Rust reconciliation coverage and Playwright coverage proving an app-server health loss removes Working, Stop, and the sidebar spinner while leaving a reconnect explanation. Full 57-test Playwright suite, Bombadil, typecheck, and Rust library checks pass.
- **Flagged:** App-server attachments are still per active conversation because existing per-session Codex homes contain provider configuration and thread history. `SESSION_LIFECYCLE.md` specifies the safe provider-supervisor migration rather than silently sharing homes or credentials.
- **CUA notes:** Poolside uses one helper/MLX sidecar across chats and labels chat state independently; Codex app-server documents durable threads, resumable connections, loaded-thread state, and terminal interrupted turns.
- **Refs:** `SESSION_LIFECYCLE.md`, `src-tauri/src/codex.rs`, `tests/playwright/session-lifecycle.spec.ts`.
