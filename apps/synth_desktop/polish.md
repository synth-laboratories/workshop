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

### 2026-08-10 — Muse residency Memory unavailable CUA locks

- **Shipped:** (none — deliberate red debt) Bombadil fixture + honesty suite for the sidebar Muse-Glimmer card that paints green-ready chrome with `Memory unavailable` and `Free scheduled … · awaiting unload`.
- **Tests:** Added `tests/bombadil/muse-residency-honesty.spec.ts` (11 red always-properties on the CUA fixture + reachability); wired `run.mjs` seed (`memoryBytes: null`, past `freeAt`), `test:bombadil:muse-residency`, and `desktop-ui-gates.sh` catalog. Confirmed exit 2 with 11 distinct honesty violations.
- **Flagged:** Product still renders `formatMemory(null)` as `Memory unavailable` under a ready dot and will claim awaiting unload after `freeAt` elapses even when resident bytes are unknown.
- **CUA notes:** 1:20 PM capture — Muse-Glimmer-30B-GGUF Memory unavailable / awaiting unload, Laguna-XS-2.1 ready underneath, MLX sidecar Monitor paused.
- **Refs:** `LocalModelResidency.tsx`, `muse-residency-honesty.spec.ts`, screenshots in session assets.

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

### 2026-08-09 — Configurable native MLX idle release

- **Shipped:** `SYNTH_LAGUNA_IDLE_UNLOAD_SECONDS` now governs the native Responses MLX backend as well as the legacy managed sidecar. The temporary local-development default is 30 seconds; native weights and prompt caches are released while the daemon remains available for the next prompt.
- **Tests:** Added a native-backend lifecycle unit test. CUA ran the isolated **Synth Desktop · beta** app against a local MLX daemon: prompt load → `20.1 GB resident` → post-response 30-second countdown → resident card removed after unload.
- **Flagged:** The countdown begins after an active generation completes; a generation is never evicted mid-response.
- **CUA notes:** A single `Reply with exactly: ready` turn ran for 40 seconds. The UI deferred its initially elapsed schedule while that turn was active, then reset the countdown from the completed turn and removed residency after the next 30 seconds.
- **Refs:** `services/laguna-daemon/laguna_daemon/config.py`, `responses_api/backends/mlx.py`, `responses_api/service.py`, `laguna_daemon/app.py`.

### 2026-08-09 — Reconcile detached local agent sessions

- **Shipped:** Conversation, turn, and runtime-attachment state are now separate. Desktop startup reconciles orphaned running turns to interrupted; Stop is idempotent when the process is already gone; unexpected app-server exit emits a health event, persists the interrupted run, and removes only its fenced attachment generation. Sending another message lazily resumes the durable thread.
- **Tests:** Added Rust reconciliation coverage and Playwright coverage proving an app-server health loss removes Working, Stop, and the sidebar spinner while leaving a reconnect explanation. Full 57-test Playwright suite, Bombadil, typecheck, and Rust library checks pass.
- **Flagged:** App-server attachments are still per active conversation because existing per-session Codex homes contain provider configuration and thread history. `SESSION_LIFECYCLE.md` specifies the safe provider-supervisor migration rather than silently sharing homes or credentials.
- **CUA notes:** Poolside uses one helper/MLX sidecar across chats and labels chat state independently; Codex app-server documents durable threads, resumable connections, loaded-thread state, and terminal interrupted turns.
- **Refs:** `SESSION_LIFECYCLE.md`, `src-tauri/src/codex.rs`, `tests/playwright/session-lifecycle.spec.ts`.

### 2026-08-09 — Real Codex process lifecycle integration suite

- **Shipped:** Codex roots and binaries are injectable below the Tauri command layer, and CoreRuntime event publication now accepts any Tauri runtime. Production behavior is unchanged; tests can run the real process manager with isolated state.
- **Tests:** Added an executable stdio JSON-RPC app-server fixture. Rust tests kill the child during a turn, assert SQLite interruption and reason, call Stop after detachment, resume the original thread in a replacement process, reconcile an orphan after restart, and prove stale EOF cannot detach a newer attachment.
- **Flagged:** This covers the actual child-process and SQLite boundary but not a real model/provider or installed WebView. Those remain a separate, slower acceptance layer.
- **Refs:** `SESSION_LIFECYCLE.md`, `testing.md`, `src-tauri/tests/fixtures/fake_codex_app_server.py`.

### 2026-08-09 — steerTurn + visual_manage dogfood + debt scrub

- **Shipped:** `window.synthCodex.steerTurn` → Rust `codex_turn_steer` / Codex `turn/steer`; composer `steerSupported` follows the bridge. MCP `visual_manage` create dogfood in Playwright: originating chat activity + registry id + pane open. `analysis.visual.v1` normalizes agent `type`/`text` blocks. Async leave-safe is projection-driven; Respond opens `intern-intervention-input` via send (no stub toast). Intern Live/Background stay out of the launch picker (lock replaces mailbox xfail).
- **Tests:** Flipped `gaps` / `poolside-polish` / `design-debt` xfails to passing locks; static `design_debt.test.mjs` updated.
- **Refs:** `tests/playwright/gaps.spec.ts`, `poolside-polish.spec.ts`, `design-debt.spec.ts`, `src-tauri/src/codex.rs`, `visuals/templates/analysis.visual.v1/shell.tsx`.

### 2026-08-09 — Poolside polish first pass

- **Shipped:** Unified `synth.preferences.v1` schema with migration from legacy keys; tool-activity Detailed/Grouped/Compact presentation; honest FIFO prompt enqueue with stop-after queue affordances; layout persistence (sidebar/output/terminal) with default/save/reset; conversation rename/pin/archive + archived Settings surface; Settings IA (General / Models / Runtime / Account / About) with theme, fonts, submission preference, shortcuts; a11y focus/hit-target/reduced-motion polish for new controls.
- **Deferred (tested):** Duplicate conversation and permanent delete omitted until product defines copy/delete semantics. Fake updater / remote-access / app-icon switcher intentionally absent. Voice Recognition (Whisper download/select + local mic STT) and slash command/skills menu are shipped. Steer ships via `turn/steer` (see entry above).
- **Architecture:** One renderer preferences module (`src/renderer/src/preferences/`) owns schema, normalize/migrate, layout, queue, and conversation meta. Model knobs stay in `modelCapabilities.ts`. Backend/env settings remain on `synthConfig`. Eval adapter: `window.__synthPreferences`.
- **Tests:** `tests/playwright/poolside-polish.spec.ts` (prefs persistence, malformed normalize, activity modes, enqueue + honest steer fail without native bridge, FIFO, conversation actions, layout reload, keyboard settings, narrow overflow). Bombadil invariants extended for mode/theme/queue/focus/sidebar/composer/settings. Static a11y suite green (46). Typecheck green.
- **Commands:** `npm --prefix apps/synth_desktop run typecheck` pass; `node --test apps/synth_desktop/tests/*.test.mjs` 46 pass; `npx playwright test … poolside-polish.spec.ts` 11 pass; `BOMBADIL_TIME_LIMIT=20s npm run test:bombadil --workspace @synth/synth-desktop` pass.
- **Flagged:** Installed-app CUA relaunch / multi-chat keyboard pass still needed on a packaged build.
- **Refs:** `HANDOFF_POOLSIDE_POLISH_FIRST_PASS.md`, `preferences/`, `tests/playwright/poolside-polish.spec.ts`.

### 2026-08-09 — Projects parked and Poolside polish hardened

- **Shipped:** Removed the premature Projects section, create-project action, project selection state, and active renderer/Rust project bridge. New conversations remain workspace-backed and projectless. Kept the existing SQLite project tables and migration history non-destructively so a future implementation can adopt an explicit contract.
- **Documented:** `project.md` records why Poolside's repo-centric Projects model does not yet fit Workshop, the future Conversation / Workspace / Project distinction, persistence boundaries, reintroduction criteria, and the required Playwright/Bombadil/CUA test bar.
- **Fixed:** Durable queued prompts are removed only after the runtime accepts them; concurrent drains are fenced per session; queue controls now receive pointer input and lay out above the composer; conversation menus support Arrow/Home/End and restore focus; Apply default and Reset layout have distinct saved-default/factory semantics; reduced-motion disables the working spinner and shortens motion globally.
- **Fixed:** Outputs no longer auto-opens a floating shelf over live transcript controls when a resource appears. The Outputs badge remains visible and the shelf opens explicitly.
- **Tests:** Added regression locks for the absent Projects surface, workspace settings, rejected queue sends, context-menu keyboard behavior, saved/default/factory layout behavior, and the closed-by-default Outputs shelf. Full Playwright suite: 75 passed. Static accessibility: 47 passed. Rust library: 119 passed. Typecheck passed. The bounded Bombadil exploration passed during the first-pass run; two later reruns explored without a property violation but hit Bombadil's process-exit watchdog after its time limit.
- **Deferred (tested):** Real steer still requires a runtime primitive. Duplicate/permanent delete remain deferred pending product semantics. Historical project storage remains readable but has no active UI or command registration. Bombadil's intermittent post-limit hang remains a harness reliability issue.
- **Installed CUA:** Rebuilt, signed, installed, and restarted `/Applications/Synth Desktop.app`. Confirmed no Projects section or project actions; General exposes theme/font/submission/activity/layout controls; an activity-mode change persisted across a real process restart; the user's original Grouped mode was restored; an existing resource-bearing chat showed `Outputs 2` collapsed with no shelf obscuring the transcript.
- **Refs:** `project.md`, `preferences/`, `tests/playwright/poolside-polish.spec.ts`, `tests/playwright/runtime-regressions.spec.ts`.

### 2026-08-09 — Bombadil visual-alignment coverage

- **Shipped:** Added a deterministic Bombadil runtime fixture containing a local chat with an attached visual, plus a focused `test:bombadil:alignment` entrypoint that opens the fixture, expands Outputs, and holds it open while mutating supported viewport sizes.
- **Invariants:** The Outputs trigger stays inside the chat with the documented right inset; the panel agrees with `aria-expanded`, shares the trigger's right edge, opens below it, stays inside the chat, clears the composer, and creates no horizontal overflow. The transcript and composer retain a common centerline.
- **Tests:** Focused Bombadil run explored 117 states across 960×640, 1280×840, and 1440×900 with the panel expanded and zero violations.
- **Refs:** `tests/bombadil/visual-alignment.spec.ts`, `tests/bombadil/layout.spec.ts`, `tests/bombadil/run.mjs`.

### 2026-08-09 — Installed-app visual-system sweep

- **CUA audit:** Reviewed the packaged landing, transcript, resource shelf, Connectors, Search, Visuals, Optimizers, Inventory containers/traces, every Settings section, inference monitor, embedded terminal, and compact split-pane states. Inventory Traces remains the strongest reference surface.
- **Shipped:** Rebuilt Optimizers around a structured toolbar, run/inspector workbench, semantic status treatments, coherent actions, and useful empty states. Activity and Outputs now share a single non-overlapping transcript toolbar. The terminal has a distinct dark canvas and chrome. The landing agent action is a compact product card, and About no longer exposes internal parity/debt notes.
- **Tests:** Typecheck and frontend build pass. Full Playwright suite: 76 passed. Rust/desktop install verification passed (125 library tests, 35 protocol tests, 3 visuals MCP tests, and all other executed targets; one real-bundle test remains intentionally ignored). Focused Bombadil alignment exploration exercised 960×640, 1280×840, and 1440×900 without a property violation before its configured time limit.
- **Installed CUA:** Rebuilt, signed, installed, and restarted `/Applications/Synth Desktop.app`. Confirmed the new Optimizers empty/run structure, compact landing agent card, dark live terminal, and separated Activity/Outputs controls in the real WebView.
- **Flagged:** The global sidebar can still become dense with many automatically titled chats, and Account/About remain intentionally sparse. Those are information-architecture/content issues rather than unresolved alignment defects.
- **Refs:** `components/OptimizersPage.tsx`, `components/ChatTranscript.tsx`, `components/LandingPage.tsx`, `components/TerminalPanel.tsx`, `components/SettingsPage.tsx`, `styles/app.css`, `tests/playwright/poolside-polish.spec.ts`, `tests/playwright/runtime-regressions.spec.ts`, `tests/bombadil/visual-alignment.spec.ts`.

### 2026-08-09 — Unified Workshop quality and style guide

- **Documented:** Consolidated Synth visual language, Poolside-inspired interaction principles, runtime/state honesty, accessibility, CUA review, viewport/state matrices, test gates, and the definition of done in [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](../../WORKSHOP_QUALITY_STYLE_GUIDE.md).
- **Architecture:** Explicitly names `app.css`, visual chrome tokens, model capability registry, renderer preferences, Rust CoreRuntime/SQLite, `testing.md`, and `polish.md` as the relevant sources of truth.
- **Refs:** `README.md`, `HANDOFF_POLISH_CUA_TESTS.md`, `HANDOFF_POOLSIDE_POLISH_FIRST_PASS.md`, `testing.md`, `visuals/chrome/tokens.css`.

### 2026-08-09 — Provisional Workshop style triage

- **Documented:** Recorded the categorical debugging rules in [`workshop_style.md`](../../workshop_style.md): trust, lifecycle, security, dead controls, layout, accessibility, chronology, hierarchy, copy, architecture smells, expected-fail flags, and the minimum proof for completion.
- **Rule:** Never ship a polished-looking lie; fix it, remove it, or flag it with a test.
- **Refs:** [`WORKSHOP_QUALITY_STYLE_GUIDE.md`](../../WORKSHOP_QUALITY_STYLE_GUIDE.md), `README.md`.

### 2026-08-09 — Queue tray and inference inspector composition

- **Shipped:** Reworked the active-turn queue into a bounded `Next turns` tray with a clear count, compact single-line rows, numbered affordances, ellipsis-safe editing, icon-only removal, and a restrained post-stop status strip. The queue now stays above the composer without widening the document or pushing the input below the viewport.
- **Shipped:** Rebalanced the local inference monitor as an inset, rounded inspector card inside a proportional rail. The panel now has consistent internal rhythm, readable metric cards, restrained separators, responsive two-column chips, and compact sparkline/recent-request sections instead of a dense full-height slab. When open, the composer now ends at the transcript edge rather than floating underneath the inspector.
- **Tests:** Queue Playwright coverage asserts ordering, bounded width, composer separation, no horizontal overflow, and ellipsis-safe rows. Native Codex stream coverage asserts inference rail/panel containment, inset spacing, composer/rail separation, and no overflow. Bombadil layout invariants cover the same panel geometry; inference component tests remain green. Typecheck, frontend build, full 76-test Playwright suite, and packaged install verification pass.
- **Installed CUA:** Rebuilt, signed, installed, and restarted `/Applications/Synth Desktop.app`. Confirmed the real ready-state inspector is inset, contained, visually balanced, and clear of the composer; live metrics render without clipping or fabricated values.
- **Flagged:** A real generating-state CUA pass remains useful for tuning live-rate density; component and browser fixtures cover the generating state without inventing values.
- **Refs:** `components/Composer.tsx`, `components/InferencePanel.tsx`, `styles/app.css`, `tests/playwright/poolside-polish.spec.ts`, `tests/playwright/runtime-regressions.spec.ts`, `tests/bombadil/layout.spec.ts`.

### 2026-08-09 — CUA fuzz: layered composer and dense-history recovery

- **CUA findings:** A long user brief could visually dominate the transcript and leave the newest live state underneath the composer. The model picker/terminal stack had no explicit visibility or hit-test contract. A large automatically titled history made the sidebar feel like an unbounded log. Local telemetry also surfaced a clearly impossible nine-digit decode rate.
- **Shipped:** Long user prompts now collapse to a compact, expandable surface; transcript bottom clearance follows the actual composer dock as the queue grows or terminal opens; model menus are bounded above the terminal; sidebar history starts compact while retaining pinned/active/working chats; implausible local decode samples are withheld; request counters have an explicit three-column row.
- **Tests:** Added a 14-chat sidebar Playwright fixture; a long-prompt active-turn geometry test; model picker viewport/terminal/hit-test checks at four sizes; `composer-surfaces.spec.ts` Bombadil exploration; renderer and daemon throughput guards. Focused Playwright: 13 passed. Focused Bombadil: no property violation through its time-bounded exploration. Laguna telemetry unit suite: 28 passed.
- **Record:** `CUA_FUZZ_INVARIANTS.md` contains the observed failures, non-vacuous invariant table, commands, and next CUA lanes.

### 2026-08-09 — CUA fuzz: dense search containment

- **CUA finding:** With enough conversations, Search visibly clipped the final result at the dialog's rounded edge, making a dense history feel unfinished even though the list was technically scrollable.
- **Shipped:** The search dialog is now a bounded flex column: its input retains a fixed row and its result list fills only the remaining height, scrolls internally, and never bleeds below the dialog.
- **Tests:** A 24-session Playwright fixture scrolls the final result into view, checks its geometry against the results and dialog bounds, proves internal scrolling/no horizontal overflow, and opens the final result.
- **Refs:** `components/ConversationSearch.tsx`, `styles/app.css`, `tests/playwright/sidebar-navigation.spec.ts`, `CUA_FUZZ_INVARIANTS.md`.

### 2026-08-09 — Working composer composition

- **CUA finding:** The active-turn composer treated its keyboard affordance as a separate full-width line and inherited a global orange textarea focus outline. The result looked like stacked, unrelated form controls rather than one message surface.
- **Shipped:** The active mode is now a small, honest toolbar status (`Queue next` / `Steer current`) with the complete behavior in its accessible label and tooltip. The text field is height-bounded and the outer composer owns the sole subdued focus treatment.
- **Tests:** The active-turn Playwright fixture focuses the actual input and asserts the compact composer/input geometry, toolbar-contained status, absent textarea outline, absent internal runtime jargon, and no horizontal overflow.
- **Refs:** `components/Composer.tsx`, `styles/app.css`, `tests/playwright/poolside-polish.spec.ts`, `CUA_FUZZ_INVARIANTS.md`.

### 2026-08-09 — Capability-driven reasoning disclosures

- **Shipped:** Local Laguna reasoning now appears as a restrained, collapsed `Thought` disclosure rather than generic tool activity. The owned MLX Responses bridge emits this stream separately from the answer, so it can be opened without contaminating assistant text.
- **Safety:** Remote and closed-model targets—including GPT 5.6 Luna—are classified as `summary` only. When their provider returns a reasoning payload, Workshop labels it `Reasoning summary · Provider summary`; it never presents it as full local thought. A target that supplies no displayable reasoning renders no empty disclosure.
- **Architecture:** `runtime/modelCapabilities.ts` now owns the display policy alongside each model's request knobs; `sessionView` applies it per durable session rather than branching in the transcript UI. This keeps new models declarative and prevents transport wording from deciding privacy semantics.
- **Tests:** Playwright covers collapsed/expandable local thought and summary-only remote Luna behavior. A responsive Bombadil specification keeps any rendered reasoning disclosure semantic, transcript-contained, composer-clear, and free of horizontal overflow across supported viewport fuzzing. Focused Playwright (2) and Bombadil passed.
- **Refs:** `runtime/modelCapabilities.ts`, `runtime/sessionView.ts`, `components/ChatTranscript.tsx`, `tests/playwright/runtime-regressions.spec.ts`, `tests/bombadil/reasoning-disclosure.spec.ts`.

### 2026-08-09 — Canonical OpenRouter credential discovery

- **Finding:** The canonical desktop used a profile-specific private env file for Synth credentials while an existing nonempty OpenRouter key remained in the original canonical private env file. The app therefore incorrectly reported OpenRouter as unconfigured despite a valid local credential being present.
- **Shipped:** Canonical installs now resolve `OPENROUTER_API_KEY` from the active private env file first, then the canonical legacy private env file only when needed. Named development instances (which set `SYNTH_DESKTOP_DATA_ROOT`) remain credential-isolated and never inherit another instance's key.
- **Tests / installed proof:** Added a Rust precedence test for active-versus-legacy credentials, rebuilt and installed the complete signed bundle, and verified via CUA that the installed app reports `OpenRouter ready`. The credential itself was never displayed or copied.
- **Refs:** `src-tauri/src/synth_config.rs`, `src-tauri/src/lib.rs`.

### 2026-08-09 — Synth Cloud local bind-address normalization

- **Finding:** A Synth Cloud Laguna S start could fail before any provider request with `baseUrl must be local HTTP or HTTPS` when a local backend advertised its bind address as `http://0.0.0.0:port`. `0.0.0.0` is valid for listening but is not a usable client address.
- **Shipped:** Synth Cloud provider setup normalizes that exact local bind address to `http://127.0.0.1:port/api/v1` before the Codex provider configuration and validation run. No remote HTTP hosts are newly permitted.
- **Tests:** Added a Rust regression test that applies the provider setup, validates the resulting start request, and asserts the exact loopback Responses endpoint.
- **Refs:** `src-tauri/src/codex.rs`.

### 2026-08-09 — Safe, actionable provider endpoint errors

- **Finding:** A provider validation failure exposed the implementation name `baseUrl` and omitted both the selected provider and the corrective path, leaving a user unable to tell whether the local service, cloud target, or a saved setting was at fault.
- **Shipped:** Endpoint validation now identifies the selected provider, shows a sanitized endpoint, explains the supported local/HTTPS forms, and directs the user to **Settings → Account → Backend API**. URL credentials and query/fragment values are redacted before the message is displayed.
- **Tests:** Rust regression test asserts the provider label and corrective path are present and that URL user-info and query tokens never enter the error text.
- **Refs:** `src-tauri/src/codex.rs`.

### 2026-08-10 — Bombadil catches blank “Worked” completed turns

- **CUA finding:** Synth Cloud Laguna S finished a turn as `Worked 11s` with a blank answer surface, a `Reasoned` marker, and composer chip text `Unavailable tok/s observed p50`.
- **Tests:** Added deliberately-red Bombadil fixture `empty-completed-turn.spec.ts` that seeds that transcript/chip state. Invariants:
  - `completed_turns_never_look_successful_when_blank`
  - `composer_never_advertises_unavailable_tok_s`
- **Confirmed:** Focused run exits non-zero with both property violations against the injected fixture (`blankSuccessfulTurn: true`, `unavailableThroughputChip: true`).
- **Harness:** Bombadil Laguna stub now includes `listModels` (App boot was crashing the headless renderer); runtime python prefers `SYNTH_PYTHON` / Laguna venv / 3.12.
- **Refs:** `tests/bombadil/empty-completed-turn.spec.ts`, `tests/bombadil/run.mjs`, `package.json` `test:bombadil:empty-turn`.

### 2026-08-10 — Bombadil catches composer toolbar wrap + throughput/Max overlap

- **CUA finding:** Toolbar showed `Never ask · Full system` with `access` stacked on a second line, and `Unavailable tok/s observed p50` colliding with the Thinking `Max` chip.
- **Tests:** Deliberately-red `composer-toolbar.spec.ts` seeds allow-all permissions + implausible throughput, selects OpenRouter Laguna S (Max), and fuzzes widths. Invariants:
  - `permission_control_never_stacks_full_system_access`
  - `throughput_never_overlaps_thinking_chip`
- **Confirmed:** Focused run reports both violations (`permissionStacksVertically: true` height 44, `modelOverlapsReasoning: true`).
- **Refs:** `tests/bombadil/composer-toolbar.spec.ts`, `package.json` `test:bombadil:composer-toolbar`, `scripts/desktop-ui-gates.sh`.

### 2026-08-09 — Terminal failure truthfulness

- **Finding:** A Responses-compatible provider can send a `turn/completed` envelope whose nested turn is actually failed. Workshop treated the envelope name as authoritative, displayed `Worked`, and left the transcript with no answer.
- **Shipped:** The Rust bridge and restored-event projection now normalize that envelope to a failed turn. When any terminal turn contains no assistant answer, the transcript renders a concise, retry-oriented explanation instead of a blank successful-looking result.
- **Tests:** Playwright regression covers the exact contradictory envelope and asserts the provider error is visible while `Worked` is absent. The focused test passes. The accompanying Rust unit is blocked only by unrelated, concurrent `sft_recipes.rs` type errors in the shared worktree.
- **Refs:** `src-tauri/src/codex.rs`, `src/renderer/src/runtime/nativeCodex.ts`, `src/renderer/src/runtime/sessionView.ts`, `tests/playwright/session-lifecycle.spec.ts`.

### 2026-08-09 — Approval policy truthfulness

- **Finding:** The composer could show `Allow all` while the in-flight app-server had already attached with Ask, allowing a later provider approval card to contradict the visible policy. Separately, a provider that asked despite `approvalPolicy: never` was rejected rather than automatically accepted.
- **Shipped:** An in-flight policy change now remains visibly attached to the current turn and is saved for the following turn. The Rust bridge auto-accepts an unexpected request under explicit `Allow all`, preferring session approval and falling back to one permitted action; it emits no modal. Restored sessions that retain only the human approval-mode field now derive their required wire policy instead of silently reverting to Ask.
- **Tests:** Focused Playwright approval-mode and terminal-lifecycle regressions pass. Rust auto-approval unit coverage is present but the workspace-wide Rust compile remains blocked by unrelated concurrent `sft_recipes.rs` type errors.
- **Refs:** `src-tauri/src/codex.rs`, `src/renderer/src/App.tsx`, `tests/playwright/runtime-regressions.spec.ts`.

### 2026-08-09 — Reasoning disclosure composition

- **CUA reference:** Poolside presents reasoning as a compact `··· Thought` disclosure with one chevron and unboxed prose only after expansion. It does not reserve a large outlined card for idle/streaming thought, nor repeat provider metadata in the visual label.
- **Shipped:** Workshop reasoning is now the same restrained disclosure pattern: no card chrome, no giant placeholder wave, one clear chevron, and readable proportional prose when opened. `Thought` remains reserved for local full reasoning; closed-model output remains labeled `Reasoning summary` in the control's accessible name.
- **Tests:** Playwright verifies collapsed/expanded local thought, closed-model summary disclosure, and absence of card chrome. Bombadil fuzzes the three responsive viewports and asserts a closed disclosure is compact, transcript-contained, card-free, composer-clear when expanded, and overflow-free.
- **Refs:** `components/ChatTranscript.tsx`, `styles/app.css`, `tests/playwright/runtime-regressions.spec.ts`, `tests/bombadil/reasoning-disclosure.spec.ts`.

### 2026-08-09 — Conversation activity marker

- **Shipped:** A running conversation has a clearly visible, Codex-style trailing activity ring in the sidebar; it stays present when the row is not selected and yields to the orange finished/unviewed dot once the turn terminates.
- **Tests:** Playwright asserts that an active conversation remains in the compact sidebar, exposes the semantic `Working` marker, and keeps the 15px ring contained at the trailing edge of its row.
- **Refs:** `styles/app.css`, `tests/playwright/sidebar-navigation.spec.ts`.

### 2026-08-09 — Live local conversation throughput

- **Shipped:** A currently decoding local Laguna conversation now carries its daemon-reported decode rate beside the Codex-style active ring in the sidebar (for example, `31.7 tok/s`). The rate is rendered only while one local running chat can be unambiguously matched to the daemon's single active generation; unavailable, implausible, queued, and ambiguous values remain absent rather than becoming a fabricated metric. Narrow sidebars retain the ring and hide only the supplementary rate.
- **Model capability check:** OpenRouter's live model metadata confirms that `poolside/laguna-s-2.1` accepts a `reasoning` object, but does not publish an effort enum. Poolside's own S 2.1 model card documents a binary `enable_thinking` control. The existing `Off` / `On` choice is therefore intentional and more accurate than presenting unverified `Low` / `Max` levels; the adapter maps that binary choice to its supported `none` / `max` transport values.
- **Tests:** A Playwright integration test supplies a real-shaped inference snapshot and asserts `Working · 31.7 tok/s`, the visible rate, and row containment. Bombadil adds invariants that any live-rate label has a working ring, a valid rate format, and remains inside its chat row.
- **Verification:** `npm run typecheck`, focused Playwright (2) and Vite production build pass. The focused 12-second Bombadil layout run exercised the new invariants without violating them; it still finds two pre-existing layout failures (`composerClearsInference` / transcript-composer centerline) in the inference-rail fuzz path, which remain separate follow-up work.
- **Refs:** `App.tsx`, `components/Sidebar.tsx`, `components/InferencePanel.tsx`, `runtime/modelCapabilities.ts`, `tests/playwright/sidebar-navigation.spec.ts`, `tests/bombadil/layout.spec.ts`.

### 2026-08-09 — Laguna S reasoning labels

- **Shipped:** The compact selector beside **Laguna S 2.1** now presents the exact supported adapter values, **None** and **Max**, instead of the ambiguous On/Off aliases. This applies consistently to both OpenRouter and Synth Cloud Laguna S targets; local Laguna XS keeps its separate On/Off control.
- **Tests:** Playwright opens the Laguna S picker, asserts its default is `Thinking: Max`, selects `None`, and confirms that the request still carries `effort: none`.

### 2026-08-09 — Persistent Outputs side panel

- Conversation views now always expose Outputs and open the floating panel by default, including before any files, visuals, or containers exist.
- Empty conversations show a quiet explanatory state; populated conversations retain the compact count and first-class output rows.
- Switching conversations restores the panel, while explicit Hide and reopen remain reversible.
- Desktop layouts reserve a real right-side lane for the panel and composer, preventing the panel from covering tool-row actions; compact layouts keep the dismissible overlay.
- Playwright covers empty, working, populated, close/reopen, ARIA state, and panel/action non-overlap behavior.

### 2026-08-09 — Quiet conversation rows

- Removed the repeated globe placeholder from every conversation row. Titles now align cleanly with Codex-style working rings and finished-unviewed dots as the only trailing status marks.
- Playwright and Bombadil lock chat rows against generic decorative icons returning.
- Fixed canonical reinstall aborting after app shutdown when no managed Laguna PID file exists; that absence is now a successful no-op.

### 2026-08-10 — Sidebar account footer

- Added a compact Codex-style account control to the bottom-left footer without conflating cloud authentication with local Laguna residency.
- Signed-out state reads `Sign in to Synth · Local mode`; authenticated state reads `Synth account · Signed in` with a green presence dot. The device-auth contract does not currently return a profile name, so the UI does not invent one; the component accepts a display name when that contract grows one.
- The account menu exposes sign-in/account management and direct logout, closes on outside click or Escape, restores keyboard focus, and stays contained inside the sidebar. Settings remains a stable one-click footer action.
- Account changes made by browser pairing, credential save, Settings logout, or footer logout update the footer immediately through a renderer account-change event.
- The mini popup now owns expandable usage, Settings, and Log out. Usage is derived from the Rust ledger: rolling seven-day tokens/cost plus all tracked tokens and entry count. The API does not expose account allowance or reset date, so weekly remaining is explicitly `Not reported` rather than fabricated.
- Typecheck and focused Playwright coverage pass: account lifecycle, sidebar navigation, and Poolside polish 27/27.

### 2026-08-10 — Dev account plan, model-picker containment, release-merge repair

- **Finding:** The v0.1 dev↔main merge left the tree unbuildable (missing `compact_before_model_switch` in `eval_driver.rs`, three renderer symbols whose definitions were stranded in stashes) and five Rust tests red because the `optimizer.run.v1` visual template and the pinned-smoke SKILL.md lines were never committed. The account footer was signed-out-only with `Weekly budget: Not reported`, and the landing model picker overflowed behind the composer (12:54 screenshot).
- **Shipped:** Merged `codex/context-compaction` (union with the permissions rework) and `agent/auth-release-fix` into `dev`; recovered stash assets; new Rust `account` module — `account_get_summary` seeds an authoritative $200/month dev plan into `runtime_settings` (never prod), charges the usage ledger cents-exact per UTC month, clamps at zero; Sidebar popup renders allowance/used/remaining/reset. Model picker now clamps to the viewport with an 8px inset, avoids the composer, flips above a low trigger, scrolls internally, closes on Escape, and reveals the selected item. First-run card no longer advertises Intern.
- **Gotcha for future work:** A `setState` inside a mount-time `useLayoutEffect` (even a bailed-out `setPlacement(null)`) perturbs StrictMode's effect replay and double-registers the app-level window keydown listeners — Cmd+J toggled twice per press and the terminal appeared dead. Never set state in the closed branch of a mount-time layout effect.
- **Tests:** 174 Rust lib tests green (6 new account tests). New `get-started.spec.ts` covers the external-download first-five-minutes path. Picker containment asserted in Playwright at 1728×1117 / 1100×700 / 960×640 and as Bombadil `always` invariants. `test-desktop-instance.sh` now compiles its unrelated-app stand-in locally (AMFI SIGKILLs copied Apple-signed binaries on Apple Silicon).
- **Open:** CUA confirmation of the installed app (signed-in footer, real plan, picker at short sizes) not yet run; hosted/prod account identity is still local-derived pending a backend identity endpoint.
- **Refs:** `src-tauri/src/account.rs`, `src/renderer/src/components/{Sidebar,LandingPage,BackendSettings}.tsx`, `tests/playwright/{get-started,account-sign-in,layout-invariants}.spec.ts`, `tests/bombadil/layout.spec.ts`.

### 2026-08-10 — Progressive-disclosure model picker

- **Shipped:** Simplified ordinary composer model rows to the user-facing model name and a fixed trailing checkmark. Removed runtime, provider id, usage, modality, context, and throughput from the default catalog scan. A collapsed Advanced section now exposes labeled details for the selected model; blocked cloud choices retain only the explanation and action needed to unblock them. The menu is narrower, shorter, sentence-case, and uses a quiet neutral selected state.
- **Tests:** Typecheck passes. Focused Synth Cloud and layout Playwright coverage passes 12/12, including short-window containment, terminal layering, names-only cloud rows, Advanced metadata, and the current Account destination for API-key setup. The broader static accessibility slice has one unrelated existing failure because `App.tsx` currently contains reachable `nativeIntern.createSession` code.
- **CUA notes:** The already-running `aesthetic-audit` native app is an installed debug bundle rather than an HMR process, so it continued to display its pre-change model menu. Browser-built Playwright exercised the updated renderer; a new native build/install is required for installed-app visual confirmation.
- **Refs:** `components/Composer.tsx`, `styles/app.css`, `tests/playwright/synth-cloud-provider.spec.ts`, `visual_style_guide_v0p1.md`.
