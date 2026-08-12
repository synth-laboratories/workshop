# Workshop v0.2 — architecture, reorg, and refactor notes

**Date:** 2026-08-11
**Scope:** `apps/synth_desktop` (36.7k Rust LOC, 18.4k renderer TS LOC)
**Sources:**
- Four-agent architecture review (full evidence, file:line):
  https://claude.ai/code/artifact/d61a3bd5-7c04-4a7b-9a55-9d5d998bfb63
- SynthStyle conformance audit + wave program:
  `jstack-dev/.jstack/style/synth_style.md` § "WORKSHOP CONFORMANCE AUDIT"
- Current-state map: [`ARCHITECTURE_MAP_2026-08-11.md`](./ARCHITECTURE_MAP_2026-08-11.md)
- SynthStyle rule that owns the noun tree:
  `prefer_hierarchies_of_clear_nouns` (also `one_authoritative_source`,
  `user_facing_copy_is_contract`, `metadata_bags_are_not_authority`,
  `domain_not_transport`)

This doc is the plan-of-record summary: **locked product nouns**, target
architecture, target file layout, and the refactor sequence. The two sources
above carry the evidence; this carries the shape. Prefer code +
`architecture.md` if this drifts.

---

## 1. The one organizing principle: clear interlanguage boundaries

Agreed and adopted as the first-class rule for v0.2. Today the Rust↔TS
boundary is the weakest structure in the product: 238 hand-synced command
strings, 9 stringly-typed event channels, ~114 hand-mirrored types across
three TS locations, errors crossing as prose, and Desktop-synthesized events
indistinguishable from provider events. Every other refactor gets cheaper
once the boundary is rigid, so the boundary law comes first:

1. **One generated contract, one direction of truth.** Rust serde types are
   the source; TS bindings are generated (tauri-specta + specta). Nothing
   hand-written mirrors a Rust type. `env.d.ts` shrinks to `Window`
   declarations; `packages/runtime-protocol` becomes generated output or is
   retired into the generated module.
2. **Command and event names are constants exported from the contract**, on
   both sides. Zero inline `"codex:event"` / `invoke("...")` literals.
3. **Errors cross as one typed `AppError`** with a stable `code` per variant,
   translated once at the Tauri edge (`informative_errors`). No
   `Result<_, String>` commands; no prose matching on either side.
4. **Events are origin-tagged.** `Event` is a `#[serde(tag)]` enum with
   `origin: Provider | Desktop`, ending the synthetic-method injection
   (`session/unhealthy`, `approval.*`) into the provider namespace. One
   emission channel per producer (the double `codex:event` + `runtime:event`
   emission is deleted).
5. **One command bus.** The `window.__synthEval` and legacy `/v1/*` HTTP
   namespaces are quarantined (test-only, tree-shaken from packaged builds)
   or deleted. The MCP bins' hand-written `inputSchema` blobs are generated
   from the same contract types.
6. **`RuntimeTarget` gets a Rust type.** The composer routing payload is a
   serde enum with a DB migration — no more renderer-owned
   `ExecutionTarget` union persisted as opaque `target_json`.

The same principle applies at the process boundary (Desktop ↔ codex
app-server ↔ Laguna daemon): typed protocol modules, not `Value` + magic
method strings.

---

## 2. Locked noun hierarchy (plan of record)

Adopted under SynthStyle `prefer_hierarchies_of_clear_nouns`: parent noun +
named children; inheritance only where behavior branches; membership and
reference edges named explicitly. One concept gets one public name
(`user_facing_copy_is_contract`). Store is substrate, not a product parent.

```text
Workspace                          (one Desktop instance / data root)
├── Identity
│   ├── Device                     (paired browser device)
│   ├── Credential                 (keys + leases; custody stays in host)
│   └── Account                    (plan, allowance, org)
│
├── Session                        ★ turn authority (one writer each side)
│   ├── SessionKind: Codex | Intern
│   ├── RuntimeTarget              (where this session runs)
│   ├── Run*                       (turns / attempts)
│   └── Event*                     (origin: Provider | Desktop)
│
├── RuntimeTarget                  ★ inference / agent substrate (Rust enum)
│   ├── LocalRuntime               → Laguna
│   ├── RemoteRuntime              → OpenRouter model ids
│   ├── CloudRuntime               → Synth gateway (via Credential lease)
│   └── InternRuntime              → sync | async
│
├── Data                           (product noun; was Inventory)
│   ├── Container*                 (loopback env / Craftax etc.)
│   ├── Trace*                     (sealed V5 subject)
│   │     └── Projection*
│   └── UsageRecord*               (one ledger)
│
├── Visual*
│   ├── Template                   (catalog)
│   ├── Instance                   (durable record + revisions)
│   └── Binding → Trace | Container | Run | inline | live
│
├── Optimizer*                     (recipes / SFT; may bind Visual + Trace)
│
└── Store                          (persistence substrate — NOT a product noun)
    ├── Database (SQLite)
    ├── ContentStore (CAS)
    └── Journal
```

### 2.1 Relations (is-a / has-a / binds / routes)

| Relation | Edge |
| --- | --- |
| **is-a** | `SessionKind` variants; `RuntimeTarget` variants; `Event` origin |
| **has-a / contains** | Workspace→Session\*; Session→Run\*; Trace→Projection\*; Identity→{Device,Credential,Account} |
| **binds / references** | Visual→Trace\|Container\|Run (by digest/id; Visual never owns Trace) |
| **runs on** | Session→RuntimeTarget |
| **routes through** | CloudRuntime turns→Credential.lease→gateway |
| **settles into** | Session→UsageRecord (Account when cloud) |
| **persisted by** | every durable noun→Store (never inverted) |

**Not domain nouns** (UI or adapters only): WorkbenchSidePanel, EvalDriver,
`threads.json` (Session cache), MCP bin process wrappers, Electron fallbacks.

### 2.2 Inheritance that earns its keep

Narrow. Only where control flow and persistence actually branch:

```text
SessionKind          RuntimeTarget           Event
  ├─ Codex             ├─ LocalRuntime         ├─ ProviderEvent
  └─ Intern            ├─ RemoteRuntime        └─ DesktopEvent
                       ├─ CloudRuntime
                       └─ InternRuntime

DataItem   (optional umbrella for the Data tab only)
  ├─ Container
  ├─ Trace
  └─ UsageRecord
```

Do **not** invent deep trees for CodexManager internals, HTTP framing, or
panel chrome. Those are collaborators under the nouns above
(`protocols_for_extension_points`: `ProviderTransport`, `SessionPersistence`,
`ManagedService` — not new product nouns).

### 2.3 Rename map (current → locked)

| Current | Locked | Notes |
| --- | --- | --- |
| Inventory / `InventoryStore` / Inventory tab | **Data** / `DataStore` / Data tab | Split repos under Data; one UI noun |
| `ExecutionTarget` (TS union → JSON bag) | **RuntimeTarget** (Rust serde enum) | Wave 2; DB columns, not `target_json` |
| `SessionTarget` (audit / early notes) | **SessionKind** (`Codex \| Intern`) | Architecture routing law; compile-checked |
| CodexManager-as-session-authority | Codex = `ProviderTransport` under **Session** | Status via `SessionService::transition` |
| LagunaManager | **LocalRuntime** implementation | Manager may keep internal name |
| CredentialBroker | **Credential** custody (lease edge) | Keep module; public noun is Credential |
| Visuals page + RH VisualPane | **Visual** | Binding graph; panel is UI membership |
| Traces under Inventory | **Data.Trace** | Sealed digest is the stable subject |
| usage_ledger + usage_records | **Data.UsageRecord** (one ledger) | Wave 7 |

### 2.4 SynthStyle conformance (why this tree)

| Rule | How the tree satisfies it |
| --- | --- |
| `prefer_hierarchies_of_clear_nouns` | Parent + children above; no flat helper soup for product concepts |
| `one_authoritative_source` | One name per concept; Session status one writer; one usage ledger |
| `metadata_bags_are_not_authority` | `SessionKind` + `RuntimeTarget` columns, not JSON key checks |
| `user_facing_copy_is_contract` | UI says Data / Session / Visual — same as Rust/TS types |
| `domain_not_transport` | Store, loopback HTTP, MCP are under nouns, not parents of them |
| `state_machines_have_explicit_transitions` | Session/Run stay in `domain/session_run.rs`; Codex adopts them |
| `umbrella_abstractions_one_layer` | `RuntimeTarget` / `ProviderTransport` are the umbrellas; no second parallel taxonomy |

---

## 3. Target architecture (what changes conceptually)

- **Session lifecycle has one authority.** `SessionKind` in `domain/`;
  Codex status flows through `SessionService::transition` exactly as Intern
  already does; `threads.json` becomes a cache, not a truth. The renderer
  mirrors this with a single `applyRuntimeEvent` reducer — one writer for
  session status on each side of the boundary.
- **Composition over god objects.** `CoreRuntime` stops being the everything
  handle (63 sites today). Consumers take the concrete `Clone` collaborators
  they need, or small capability traits. First two traits:
  `ProviderTransport` (AppServer) and `SessionPersistence` (deletes the
  `Option<Arc<CoreRuntime>>` + 14 conditional-persistence branches in codex).
- **Managed services, one vocabulary.** A `ManagedService` trait
  (spawn/probe/wait_ready/stop/restart) + supervisor registry covering the
  six bespoke lifecycles, drained on `RunEvent::ExitRequested` (which does
  not exist today). One `http_client()` factory; one hyper-based
  `LoopbackJsonServer` replacing both hand-rolled HTTP servers; one
  `mcp_stdio` transport for the three bins; one `RecipeRunner` for the two
  recipe modules.
- **Renderer gets a store layer.** The proven `preferences/` pattern
  (versioned schema, observable store, typed actions) promoted to
  `sessionStore` + selectors via `useSyncExternalStore`. App.tsx becomes
  shell + routes. Per-session memoized view slices end the
  O(sessions×events)-per-token recompute.
- **Operational parameters live in `limits.rs` / `limits.ts`.** No inline
  timeout/cap literals; no silent library defaults.
- **Usage has one ledger.** Legacy `usage_ledger` rows migrate into
  `usage_records` (`Data.UsageRecord`); the three divergent reconciliation
  queries are deleted; Laguna-local turns start writing usage rows (or the
  exemption is documented).

---

## 4. Target file layout

Layout follows the locked nouns. Module names match public vocabulary where
a product noun is the owner; Store / ipc / services stay substrate.

### 4.1 Rust crate (`apps/synth_desktop/src-tauri/src/`)

```text
src/
├── lib.rs                  composition root + generate_handler ONLY (~300 lines)
├── error.rs                AppError enum, stable codes, From<anyhow::Error>
├── limits.rs               every timeout/retry/cap/TTL, named
├── config.rs               cached AppConfig (ArcSwap + watch), Profile enum
│                           (absorbs synth_config resolution; one env authority)
├── contract/               ★ the interlanguage boundary (codegen source)
│   ├── mod.rs              specta-annotated DTOs shared with renderer
│   ├── events.rs           event channel names + payload enums (origin-tagged)
│   └── commands.rs         command arg/return types
├── commands/               thin #[tauri::command] delegations, ≤6 lines each
│   ├── session.rs · identity.rs · data.rs · runtime.rs · visuals.rs
│   ├── optimizers.rs · workspace.rs · terminal.rs · settings.rs
│   └── (hydrate_container etc. move INTO owning modules, not here)
├── domain/
│   ├── session_run.rs      (existing paragon — unchanged)
│   ├── session_kind.rs     ★ SessionKind (Codex | Intern), routing law
│   ├── runtime_target.rs   ★ RuntimeTarget enum + serde + DB mapping
│   └── ids.rs              typed id newtypes
├── identity/               Device · Credential · Account
│   ├── device_auth.rs · credential_broker.rs (injected; no globals)
│   └── account.rs · account_cloud.rs
├── data/                   ★ was inventory.rs — Container / Trace / Usage
│   ├── store.rs · containers.rs · traces.rs · usage.rs
│   └── (trace_ingest helpers live beside traces)
├── storage/                Store substrate: Database + CAS + Journal
│   ├── database.rs · migrations.rs · content_store.rs · event_journal.rs
│   └── usage_records.rs (absorbs model_performance.rs) · repositories…
├── session/                Codex + Intern as SessionKind transports
│   ├── codex/              split of today's codex.rs (4,557 → ~5 modules)
│   │   ├── proto.rs · manager.rs · event_pump.rs · telemetry.rs · home.rs
│   └── intern/             (existing cloud/intern; PollerHandle stays paragon)
├── runtime/                LocalRuntime (Laguna) + ManagedService sidecars
│   ├── laguna.rs · whisper.rs · http.rs (client factory)
│   └── supervisor.rs       ★ ManagedService trait + registry + shutdown drain
├── visuals/                Template · Instance · Binding · IPC router
├── optimizers/
│   ├── service.rs (~450) · repository.rs · projection.rs (pure)
│   ├── runner.rs           ★ RecipeRunner shared harness
│   └── fixtures.rs         #[cfg(feature = "demo")]
├── ipc/
│   ├── loopback_server.rs  ★ shared hyper loopback server
│   ├── visuals_router.rs · eval_driver_router.rs   (routers only; EvalDriver ≠ noun)
│   └── mcp_stdio.rs        ★ shared MCP transport (bins link the lib crate)
├── workspace_scope.rs      persist-then-fence invariant expressed ONCE here
└── bin/                    MCP bins reduced to tool tables + mcp_stdio calls
```

### 4.2 Renderer (`apps/synth_desktop/src/renderer/src/`)

```text
src/
├── App.tsx                 shell + layout only (~250 lines)
├── routes.tsx              MainView → component table, React.lazy pages
├── limits.ts
├── generated/
│   └── protocol.ts         ★ tauri-specta output — the ONLY Rust-type mirror
├── bridge/
│   ├── index.ts            typed invoke/listen against generated contract;
│   │                       imported (injectable), NOT window.* globals
│   └── fixtures.ts         browser/test fallback, import.meta.env-guarded,
│                           tree-shaken from packaged builds
├── stores/
│   ├── sessionStore.ts     ★ single-writer applyRuntimeEvent reducer +
│   │                         useSyncExternalStore selectors
│   └── accountStore.ts     (Identity.Account)
├── runtime/                pure logic (no React) — selectors over nouns
│   ├── sessionView.ts      per-session memoized slices + UNIT TESTS
│   ├── sessionOrchestrator.ts · promptQueue.ts · evalApi.ts
│   ├── nativeCodex.ts · modelCapabilities.ts · modelSwitchPlan.ts
├── hooks/                  useClickOutside · useVoiceInput · useInferenceMonitor
├── components/             presentational; Composer ≤10 props, Sidebar ≤10
│                           DataPage (was InventoryPage) · VisualsPage · …
├── preferences/            unchanged (paragon)
└── env.d.ts                Window declarations only (<100 lines)
```

### 4.3 Repo level

```text
DELETE      packages/runtime-client (219 lines, 0 importers)
RETIRE      packages/runtime-protocol → generated/protocol.ts (or becomes the
            generated package; types rename ExecutionTarget → RuntimeTarget)
QUARANTINE  apps/mock · apps/_ref_first_pass · services/local-runtime ·
            services/local-inference (per ARCHITECTURE_MAP §10)
DECIDE      contracts/research-v1.json — write the promised codegen or delete
```

---

## 5. Refactor sequence (waves)

Full detail + grep-able conform checks + updater-agent protocol in the
SynthStyle WORKSHOP CONFORMANCE AUDIT section. Summary (noun-aligned):

Wave 0 enforcement entrypoint (local + CI): `./scripts/desktop.sh conform`
(or `./scripts/conform-desktop.sh`). Prints labeled CONFORM CHECK counts for
`apps/synth_desktop`; each count may only decrease. CI:
`scripts/ci/desktop-conform.yml` (copy into `.github/workflows/` with a token
that has `workflow` scope). Paragons for this scaffolding lane:
`objective_tests`, `real_fixtures`.

| Wave | Work | Done when |
| --- | --- | --- |
| 0 | CI runs `desktop.sh verify` + conform-count script; paragons registered; grep-tests reclassified as lints | counts printed on every PR |
| 1 | Session authority: `SessionKind`, Codex through `transition`, renderer reducer | magic status strings outside `domain/` = 0; reconciliation shims deleted |
| 2 | Boundary codegen: tauri-specta, origin-tagged `Event`, `RuntimeTarget`, const names | `env.d.ts` <100 lines; zero hand-written invoke strings; CI fails on drift |
| 3 | Renderer store + App.tsx diet + per-session memoization | App.tsx <400; Composer ≤10 props; token event re-renders one slice |
| 4 | Split codex under `session/`; `ProviderTransport` + `SessionPersistence`; start Data/Identity module move | spawn_server ≤5 params; commands ≤6-line delegations |
| 5 | Shared infra: supervisor + exit drain, LoopbackJsonServer, mcp_stdio, http factory, limits | zero hand-rolled HTTP framing; every task owned; quit drains |
| 6 | `AppError` taxonomy; kill substring classification | `map_err(to_string)` = 0; `.to_string().contains(` on error paths = 0 |
| 7 | One `Data.UsageRecord` ledger; Laguna usage rows; Inventory→Data rename complete in UI + commands | one read model serves dashboard/allowance/feed; no user-facing "Inventory" |

Lanes: 1, 2, 3 are independent and can run in parallel; 4 is cheaper after
1; 0 first, always. Wave 2 has same-day interim wins that don't wait for
specta (const event names, typed invoke map, `RuntimeTarget` enum).

Synonyms still present in the SynthStyle audit text (`SessionTarget`,
`ExecutionTarget`, `Inventory`): treat as historical; this section is
authoritative for new work. Update the audit on the next style-doc pass.

---

## 6. In-repo paragons (imitate these, don't invent)

| Paragon | Pattern |
| --- | --- |
| `domain/session_run.rs` | typed transitions + commit-then-broadcast |
| `cloud/intern/poller.rs` `PollerHandle` | cancellable, awaited background task |
| `credential_broker.rs` `serve()` path | real hyper loopback server (its globals are a gap, not part of the paragon) |
| storage repository shape | `Arc<Database>` + pure `&Connection` free fns |
| `codex.rs` test harness (`with_paths` + `fake_codex_app_server.py`) | constructor injection vs a subprocess protocol |
| `preferences/` | versioned schema + observable store + typed actions |
| `InferencePanel.tsx` | pure reducer + injectable transport + hook |
| `CodexTurnFailure` | typed cross-boundary error with stable code union |
