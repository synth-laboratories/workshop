# Synth Desktop testing

How product surfaces are covered today, how to run each suite, and what is still
untested during the Rust CoreRuntime migration.

Ignore `apps/mock` — it is UX pin-down only and is not part of these gates.

This map is regenerated from the workspace `package.json` scripts table. The
Playwright tree currently has **0** `test.fail` / skip / fixme markers.

## Scripts table (root `package.json`)

| Script | What it runs |
| --- | --- |
| `desktop:check` | `./scripts/desktop.sh check` — TypeScript + Rust compile contracts |
| `desktop:build` | `./scripts/desktop.sh build` — production bundle, no test suite |
| `desktop:verify` | `./scripts/desktop.sh verify` — typecheck, Rust tests, instance acceptance, Playwright |
| `desktop:verify:fast` | same as `desktop:check` |
| `desktop:install` / `desktop:install:release` | `./scripts/desktop.sh install` / `install-release` |
| `desktop:instance*` / `desktop:codex:*` | `./scripts/desktop-instance.sh` / `test-desktop-instance.sh` |
| `desktop:ui-gates` / `:bombadil` / `:playwright` | `./scripts/desktop-ui-gates.sh` |
| `desktop:v02-e2e` | `./scripts/v02-e2e-gates.sh` |
| `typecheck` | `npm run typecheck --workspace @synth/synth-desktop` |
| `test` | `test:rust` + `test:visuals` + `test:a11y` + `test:ui` |
| `test:rust` | `cargo test` via `@synth/synth-desktop` `rust:test` |
| `test:a11y` | `node --test apps/synth_desktop/tests/*.test.mjs` |
| `test:visuals` | `node --experimental-strip-types --test visuals/tests/*.test.mjs` |
| `test:ui` / `test:playwright` | frontend build + Playwright config |
| `test:legacy-bombadil` | frontend build + Bombadil runner |
| `test:legacy-runtime` | Python unittest under `services/local-runtime/tests` (contract reference only) |
| `test:modern-stack` | `python3 -m unittest scripts.tests.test_modern_stack_dogfood` |
| `check:graph` / `build:graph` | Turborepo typecheck / frontend:build |
| `cache:rust:stats` | `sccache --show-stats` |

Desktop workspace (`apps/synth_desktop/package.json`) also exposes
`frontend:build`, `typecheck`, `rust:check` / `rust:test`, `test:playwright`,
`test:ui-gates*`, and directed Bombadil specs (`test:bombadil:*`).

## Quick commands

From the workshop root:

```bash
# Tranche 0 — one focused test/spec while iterating
cd apps/synth_desktop && npx playwright test tests/playwright/<spec>.spec.ts --grep '<case>'

# Tranche 1 — compile contracts, parallel and normally under a few seconds warm
npm run desktop:check

# Build tranche — production bundle only; deliberately no test suite
npm run desktop:build

# Tranche 2 — full release/CI acceptance battery
npm run desktop:verify

# Full product test umbrella
npm test

# UI gates
npm run test:playwright          # build frontend path + Playwright
npm run test:legacy-bombadil     # legacy browser/Python compatibility exploration
npm run test:a11y                # static testid / bridge / contract-authority checks

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

## Standard tranches

| Tranche | Contents | Required for |
| --- | --- | --- |
| 0: Focused | Only the nearest Playwright spec, Node test, or Rust test filter | Every edit/iteration |
| 1: Check | TypeScript `tsc --noEmit` and Rust `cargo check`, run concurrently | Handoff and cross-boundary changes |
| Build | Typecheck overlapped with the actual Tauri release build; no tests and no redundant `cargo check` | Local bundle/install |
| 2: Release | Typecheck, all Rust tests, desktop-instance acceptance, and full Playwright | Release PRs, release cuts, CI, broad integration changes |

`desktop:install` consumes the Build tranche. It does not implicitly run Tranche
1 or 2 because the release build already compiles Rust, and repeating
`cargo check` adds latency without increasing packaging confidence. Use
`desktop:install:release` when installation must be gated on Tranche 2.

Turborepo caches deterministic workspace tasks and supplies the task graph.
Rust compilation stays under Cargo with an automatically detected `sccache`
wrapper. Inspect it with `npm run cache:rust:stats`; final Tauri bundling and
signing are intentionally never restored from task cache.

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
Tauri or a real MLX/Codex process. All cases are plain `test(...)` (0 expected-fail).

| Spec | What it asserts |
| --- | --- |
| `layout-invariants.spec.ts` | Composer inside viewport at 3 sizes; stays anchored on landing scroll; no horizontal overflow; terminal toggle keeps landing visible |
| `runtime-regressions.spec.ts` | Laguna ready/starting/loading UX; remote/cloud escape when local blocked; Settings models + multi-agent; Subagents visual; projectless workspace; residency countdown; streamed Codex deltas + Stop |
| `visuals-registry.spec.ts` | Visuals library by `visual_id`; chat card = pane = registry; create draft; Outputs shelf restore |
| `gaps.spec.ts` | Remaining migration coverage (CoreRuntime diagnostics, journal replay, VisualHost, inventory) — all must-pass |
| `design-debt.spec.ts` | Intended design locks (no deferred-adapter UI or fixture catalog; Craftax Attach; Trace import; typed Laguna reload; no stub Account/Downloads/Expand chrome) |

Further specs in the same directory cover account, approvals, usage, training,
OAuth, Whisper, Mander, diagnostics, and v0.2 surfaces. Inventory via
`ls apps/synth_desktop/tests/playwright/*.spec.ts`.

**Strength:** fast regression for renderer behavior.
**Weakness:** fixtures can pass while Rust CoreRuntime / real Codex is broken.

### Codex process-boundary lifecycle

`cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --lib codex::tests`
spawns `tests/fixtures/fake_codex_app_server.py` through the production stdio
JSON-RPC process path. Unlike renderer stubs, these tests kill a real child and
assert the temporary SQLite session/run records, durable interruption reason,
thread resume request, idempotent Stop behavior, and attachment-generation
fence. They do not load a real model or exercise an installed WebView bundle.

Static companion: `tests/design_debt.test.mjs` (picked up by `npm run test:a11y`) greps for stub toast strings, leave-safe hard-wire, Craftax VisualHost heuristics, and asserts deferred adapter fixtures, UI, and styling stay absent. Specta lock: `tests/contract_single_authority.test.mjs` (zero `invokeCommand<` in `desktopBridge.ts`; no duplicate type names vs `generated/protocol.ts`).

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

Directed Bombadil specs are the `test:bombadil:*` scripts on
`@synth/synth-desktop` (`launch-debt`, `alignment`, `composer-surfaces`,
`reasoning`, `empty-turn`, `composer-toolbar`, `terminal`, `v0.1-visuals`,
`approval`, `grouped-visual`, `min-width`, `mander`).

### 3. Accessibility / surface static checks — `tests/a11y_surface.test.mjs`

Node `node:test` that greps renderer source for stable `data-testid`s, target
kinds, Tauri bridge command names (from `generated/protocol.ts`), and native
Codex restore/sequence patterns. Catches accidental removals without starting a
browser.

### 4. Rust — `apps/synth_desktop/src-tauri`

`cargo test` covers:

- Event journal append, session sequences, remote dedupe
- Content-addressed store idempotence
- Shared `AppEvent` / visual fixture contract round-trips
- Existing Laguna / config / terminal / runtime path unit tests
- `export_specta_protocol_bindings` (generated `protocol.ts` must match export)

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

## Remaining thin coverage (not expected-fail)

These are still testid-only, package-only, or manual. They are ordinary gaps,
not `test.fail` debt:

1. Full `window.synthCore` / `runtime:event` journal replay in the browser harness
2. Intern Live without the Python product runtime
3. Inventory (traces/containers/usage) UX beyond testids
4. Legacy Python SQLite migration receipt
5. Installed Tauri + real Codex/provider E2E (manual dogfood)
6. Bombadil specs beyond composer/shell invariants

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
| `export_specta_protocol_bindings` fail | regenerate `generated/protocol.ts` via ignored `regenerate_protocol_bindings` |

## CI-shaped order (recommended)

1. `npm run typecheck --workspace @synth/synth-desktop`
2. `npm run rust:test --workspace @synth/synth-desktop`
3. `npm run test:a11y`
4. `npm run test:visuals`
5. `npm run frontend:build --workspace @synth/synth-desktop`
6. Playwright

Prefer frontend-only build for UI gates during CoreRuntime work; reserve full
`tauri build` for packaging / native dogfood.
