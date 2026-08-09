# Synth Desktop testing

How product surfaces are covered today, how to run each suite, and what is still
untested during the Rust CoreRuntime migration.

Ignore `apps/mock` — it is UX pin-down only and is not part of these gates.

## Quick commands

From the workshop root:

```bash
# Full product test umbrella
npm test

# UI gates
npm run test:playwright          # build frontend path + Playwright
npm run test:legacy-bombadil     # legacy browser/Python compatibility exploration
npm run test:a11y                # static testid / bridge surface checks

# Backend / packages
npm run test:legacy-runtime      # legacy Python contract-reference tests
npm run test:visuals             # visuals registry node:test
npm run rust:test --workspace @synth/synth-desktop
# or:
(cd apps/synth_desktop && npm run rust:test)

# Faster local UI loops (skip Tauri native build)
npm run frontend:build --workspace @synth/synth-desktop
npx playwright test --config apps/synth_desktop/playwright.config.ts
node apps/synth_desktop/tests/bombadil/run.mjs
```

Artifacts land under `apps/synth_desktop/test-results/{playwright,bombadil}/`.

---

## Coverage map by product surface

| Surface / feature | Playwright | Bombadil | Other | Status |
| --- | --- | --- | --- | --- |
| Shell: sidebar + titlebar | layout | always invariant | a11y testids | covered |
| Landing page + no horizontal overflow | layout | always invariant | a11y | covered |
| Composer visibility / usable size | layout | always invariants | a11y | covered |
| Composer vs visual pane overlap | layout | always invariant | — | covered |
| Viewport sizes 960×640 / 1280×840 / 1440×900 | layout | explore actions | — | covered |
| Terminal panel toggle (`⌘J`) | layout | — | — | stubbed (browser says “available in desktop app”) |
| Laguna readiness → composer enabled | runtime | — | — | stubbed `window.synthLaguna` |
| Laguna starting/loading menu copy (no fake % download) | runtime | — | — | stubbed |
| Blocked local does not trap Luna / Intern selection | runtime | — | — | stubbed |
| Settings → Models folder discovery / choose | runtime | — | — | stubbed |
| Settings → multi-agent V1/V2/Reset | runtime | — | — | stubbed `synthConfig` |
| Sidebar model residency + scheduled free time/countdown | runtime | — | — | stubbed |
| Native Codex streaming deltas → one assistant message | runtime | — | — | stubbed `synthCodex` |
| Working… / Stop generating interrupt | runtime | — | — | stubbed |
| Subagents visual lifecycle (active → done) | runtime | always if pane present | — | stubbed Codex events |
| Projectless Codex default workspace | runtime | — | a11y (nativeCodex strings) | stubbed + `__synthEval` |
| Inventory page | — | — | a11y testid only | **thin** |
| Cloud desk | — | — | a11y testid only | **thin** |
| Visuals library / registry CRUD | visuals-registry | — | Rust registry + synthVisuals | covered |
| Visual MCP create → chat + pane + registry | — | — | Rust MCP + IPC (`visual.show`) | partial (dogfood gap) |
| Shared VisualHost | visuals-registry | — | VisualHost.tsx | covered |
| Rust CoreRuntime journal / CAS / diagnostics | — | — | `cargo test` storage + contracts | covered (unit) |
| `runtime:event` / `window.synthCore` projection | — | — | bridge a11y-ish strings pending | **gap** |
| Intern sync / async live paths | — | — | Python runtime tests (partial) | **gap for UI** |
| Traces / containers / usage inventory | — | — | Python inventory tests | **gap for UI** |
| Legacy Python → Rust DB migration | — | — | — | **gap** |
| Codex process crash/restart lifecycle | — | — | Rust real-subprocess fixture + SQLite | covered |
| Full installed Tauri + real Codex/provider E2E | — | — | manual dogfood | **gap** |

---

## Suite details

### 1. Playwright — `apps/synth_desktop/tests/playwright/`

Runs against a Vite-served renderer with `window.synthRuntime` / `synthLaguna` /
`synthCodex` / `synthConfig` stubs (`browser.fixture.ts`). Does **not** launch
Tauri or a real MLX/Codex process.

| Spec | What it asserts |
| --- | --- |
| `layout-invariants.spec.ts` | Composer inside viewport at 3 sizes; stays anchored on landing scroll; no horizontal overflow; terminal toggle keeps landing visible |
| `runtime-regressions.spec.ts` | Laguna ready/starting/loading UX; remote/cloud escape when local blocked; Settings models + multi-agent; Subagents visual; projectless workspace; residency countdown; streamed Codex deltas + Stop |
| `visuals-registry.spec.ts` | Visuals library by `visual_id`; chat card = pane = registry; create draft |
| `gaps.spec.ts` | Migration backlog (`test.fail` + some landed fixtures) |
| `design-debt.spec.ts` | **Intended design locks** (no deferred-adapter UI or fixture catalog; Craftax `:8098` Attach; Trace import control; typed Laguna reload) + **`test.fail` debt** (Account menu/Downloads/Expand stubs, inert Always-ask, Set up agent, Async Respond, always-on leave-safe, persisted analysis-visual render failure, browser Attach/Open-trace dogfood) |

**Strength:** fast regression for renderer behavior.  
**Weakness:** fixtures can pass while Rust CoreRuntime / real Codex is broken.

### Codex process-boundary lifecycle

`cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib codex::tests`
spawns `tests/fixtures/fake_codex_app_server.py` through the production stdio
JSON-RPC process path. Unlike renderer stubs, these tests kill a real child and
assert the temporary SQLite session/run records, durable interruption reason,
thread resume request, idempotent Stop behavior, and attachment-generation
fence. They do not load a real model or exercise an installed WebView bundle.

Static companion: `tests/design_debt.test.mjs` (picked up by `npm run test:a11y`) greps for stub toast strings, leave-safe hard-wire, Craftax VisualHost heuristics, and asserts deferred adapter fixtures, UI, and styling stay absent.

### 2. Bombadil — `apps/synth_desktop/tests/bombadil/`

Property-style browser exploration (`@antithesishq/bombadil`) over a built
renderer (`dist/` preferred, `out/renderer` fallback) plus an isolated Python
`local-runtime` for `/__runtime` proxying.

| Invariant | Meaning |
| --- | --- |
| `composer_exists_when_expected` | Landing/chat surfaces keep composer + input |
| `composer_is_fully_visible` | Composer rect stays in viewport |
| `composer_remains_usable` | Minimum composer/input geometry |
| `composer_does_not_overlap_visuals` | Composer does not sit under the right pane |
| `shell_never_overflows_horizontally` | No horizontal scroll |
| `core_shell_stays_visible` | Sidebar + titlebar present |
| `subagent_visual_preserves_lifecycle_groups` | If Subagents pane exists, Active/Done groups + row statuses |
| `renderer_has_no_uncaught_errors` | No uncaught exceptions |
| `renderer_has_no_console_errors` | No `console.error` |
| `exploreViewportSizes` | Actions that resize through 960 / 1280 / 1440 |

Default run: headless, 10s time limit, exit on violation.

`npm run test:bombadil:launch-debt --workspace @synth/synth-desktop` is an
intentionally-red directed CUA regression: it opens a persisted
`analysis.visual.v1` payload and fails until the renderer accepts its
agent-authored `type` blocks (or rejects them before the preview is rendered).

### 3. Accessibility / surface static checks — `tests/a11y_surface.test.mjs`

Node `node:test` that greps renderer source for stable `data-testid`s, target
kinds, Tauri bridge command names, and native Codex restore/sequence patterns.
Catches accidental removals without starting a browser.

### 4. Rust — `apps/synth_desktop/src-tauri`

`cargo test` covers:

- Event journal append, session sequences, remote dedupe
- Content-addressed store idempotence
- Shared `AppEvent` / visual fixture contract round-trips
- Existing Laguna / config / terminal / runtime path unit tests

Commands under test for CoreRuntime: `core_diagnostics`, `core_events_after`,
`core_session_events_after`, channel `runtime:event`.

### 5. Legacy Python contract reference — `services/local-runtime/tests/`

Unittest for store, HTTP API, inventory, Intern client, Codex session helpers,
config. This suite is retained for migration/contract comparison only; it is
not part of the desktop product gate and Desktop never starts this service.

### 6. Laguna / inference — `services/laguna-daemon/tests`, `services/local-inference/tests`

Responses shim and inference unit coverage for the MLX sidecar path.

### 7. Visuals package — `visuals/tests/registry.test.mjs`

Template registry discovery / metadata for bundled visuals.

---

## Gaps we should add next (aligned with Rust migration)

Tracked as expected-fail Playwright cases in
`apps/synth_desktop/tests/playwright/gaps.spec.ts` so a normal UI run prints the
backlog:

1. **`window.synthCore` / diagnostics in browser harness**
2. **`runtime:event` journal replay after reload**
3. **Visuals library list by `visual_id`**
4. **Shared VisualHost across chat / registry / right pane**
5. **MCP `visual_create` → one registry**
6. **Intern Live without Python product runtime**
7. **Inventory (traces/containers/usage) from Rust**
8. **Legacy Python SQLite migration receipt**

Also still thin (testid-only or package-only): Inventory page UX, Cloud desk UX,
Bombadil specs beyond composer/shell.

---

## Interpreting failures

| Failure mode | Likely cause |
| --- | --- |
| Playwright timeout on `runtime-status` | Vite not up / fixture init script broke App boot |
| Composer overlap / overflow | CSS layout regression |
| Subagents / streaming failures | `sessionView` / Codex event projection regressions |
| Bombadil “No renderer build” | Run `npm run frontend:build --workspace @synth/synth-desktop` |
| Legacy Bombadil runtime not healthy | Explicit compatibility fixture import/path/`connection.json` |
| Rust contract fixture fail | `packages/runtime-protocol/fixtures` drift vs serde models |

---

## CI-shaped order (recommended)

1. `npm run typecheck --workspace @synth/synth-desktop`
2. `npm run rust:test --workspace @synth/synth-desktop`
3. `npm run test:a11y`
4. `npm run test:visuals`
5. `npm run frontend:build --workspace @synth/synth-desktop`
6. Playwright

Prefer frontend-only build for UI gates during CoreRuntime work; reserve full
`tauri build` for packaging / native dogfood.

## Latest local run (2026-08-09)

| Suite | Result |
| --- | --- |
| Playwright (layout + runtime + visuals + gaps + **design-debt**) | **passed** (design-debt: 4 locks + 9 expected-fail) |
| Bombadil `layout.spec.ts` | **passed** (no invariant violations) |
| a11y surface + **design_debt.test.mjs** | **passed** |
| Rust lib tests | see `npm run rust:test` |
