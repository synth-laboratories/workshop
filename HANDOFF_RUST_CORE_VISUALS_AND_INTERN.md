# Handoff: Rust CoreRuntime · Visuals · Intern SDK

**Date:** 2026-08-08  
**Repo:** `workshop/`  
**Audience:** engineer dogfooding and hardening the Rust-owned Desktop runtime  
**Status:** packaged Desktop cutover, real-user legacy migration, and installed-app Intern transport dogfood are complete; staging did not emit worker/agent output, which remains the external release gate  

Related:
- Prior plan: Codex outputs `synth-rust-core-runtime-and-visual-registry-handoff.md`
- Intern slot IA: `apps/synth_desktop/HANDOFF_INTERN_LOCAL_SLOT.md`
- Architecture invariants: `architecture.md`
- Testing map: `testing.md`
- Python Intern client today: `services/local-runtime/.../intern_client.py` + `adapters/intern.py`
- Canonical Python SDK: `synth-ai` → `synth_ai/sdk/research/research_intern.py` (+ MCP research tools)

---

## 0. One-liner

> Tauri/Rust is the product authority. React only projects. Python is packaged only for Laguna/MLX Responses. Visuals, projects, inventory, Codex persistence, and Intern mailbox normalization now live behind the Rust CoreRuntime and unified journal.

### Execution decision

This document is the authoritative scope for the next implementation sequence. The order is intentional:

```text
stabilize Rust core
  → unify visual events and bindings
  → establish the template/eval platform
  → finish shared session/run services
  → port Intern to Rust
  → migrate legacy data and transports
  → remove the Python product runtime
  → expand the production template catalog
```

Do not build the Rust Intern path directly on the current transitional seams. The event journal, post-commit broadcast, session/run authority, and visual binding contract must become shared infrastructure first.

### Implementation checkpoint — 2026-08-09

Completed:

- one `CoreRuntime` composition root, SQLite/CAS authority, global journal, and single post-commit Tauri event forwarder
- atomic project, session, run, receipt, inventory, and visual mutations with rollback coverage
- shared `SessionService` / `RunService`, durable cursors, restart reconciliation, and Codex adaptation
- typed Rust Intern sync/async HTTP client, pollers, replay dedupe, generation fencing, command receipts, and Tauri commands
- semantic command receipts (`received|delivered|applied|noop` complete; `refused|superseded|conflict` reject) with failed-run cleanup and persisted remote evidence
- one durable Rust-backed async singleton binding, deterministic legacy-duplicate handling, session-scoped remote event IDs, and migrated-demo/live projection precedence
- Desktop Cloud transport cut from Python HTTP to `window.synthIntern`; projects use `window.synthProjects`
- centralized Rust Visual Registry, authenticated MCP loopback, chat cards, right `VisualHost` pane, Visuals vault, revisions, and nine manifest-driven templates
- Rust-owned containers, traces, usage, diagnostics, and health projection
- explicit legacy migration in Settings: scan → inspect → typed confirmation → backup/import → receipt; source mutation is fenced and the source is never written/deleted
- Python `RuntimeManager`, spawn/proxy commands, renderer bridge, runtime-client dependency, and packaged `services/local-runtime` resource removed
- packaged Python is now only `services/laguna-daemon` for MLX/Responses
- stale Tauri command mismatches now instruct a full app quit/reopen instead of presenting an opaque missing-command failure

Green verification (latest complete pass before the final focused migration-edge regression):

- Rust suite passed; current inventory is 80 library tests + 35 Intern/protocol/integration tests, with the final migration/demo edge regression rerun directly; one real Trace V5 bundle test remains opt-in
- TypeScript typecheck; 4 visual catalog tests; 26 accessibility/source-contract tests
- production frontend/release build; Playwright 41/41
- retired Python runtime contract tests 21/21; `git diff --check` clean
- macOS `.app` and DMG release build; bundle inspection contains Visuals + Laguna and no Python local-runtime

Real migration dogfood completed on 2026-08-09: 812/812 records imported from the legacy database, with a retained consistent snapshot and receipt, `integrity_check=ok`, zero foreign-key violations, unchanged source metadata, and migrated projects/sessions/visuals visible after restart.

Installed-app staging dogfood also completed on 2026-08-09. The API key is held only in the private `0600` `~/.synth-desktop/.env`; TOML contains the staging profile and endpoint, not the secret. Sync create/message/pause/resume, semantic receipts, mailbox polling, cursor persistence, restart reconciliation, and UI status projection all worked through Rust. Async ensure reused the organization singleton, checkpoint instruction, resume, pause, event ingestion, one-local-binding enforcement, and restart reconciliation also worked. SQLite remained `integrity_check=ok` with no foreign-key violations, and no Python product-runtime process was present.

One external gate remains: staging accepted the sync objective/operator message and async instructions, but emitted no `agent_message`, checkpoint-created, or other worker result. Sync settled at `waiting_for_operator`; async advanced through `async_instruction_queued` and `async_resumed_with_instructions` but produced no worker event before it was safely paused. This is now isolated from Desktop transport/storage: the Rust client received successful semantic receipts and ingested every remote event/cursor the service exposed.

### In scope

- one authoritative Rust `CoreRuntime`
- atomic SQLite domain mutations plus journal events
- one normalized `runtime:event` replay/live path into React
- Rust-managed projects, sessions, runs, approvals, inventory, traces, usage, and visuals
- Rust visual MCP adapter backed by the same registry
- canonical visual template manifest and binding-resolution platform
- first generic eval template family
- Rust Research Intern client for the Desktop-used sync/async mailbox subset
- legacy Python-runtime data migration, transport cutover, packaging removal, and rollback
- Python retained only for Laguna/MLX Responses inference

### Out of scope

- organization-wide/cloud Visual Registry publication
- cross-machine visual synchronization
- full `synth-ai` SDK parity beyond Desktop-used Intern operations
- rendering React/TSX in Rust
- moving MLX inference into Rust
- allowing MCP or React to open SQLite or write arbitrary files

---

## 1. UX targets (locked from screenshots)

Two product surfaces are correct and complementary — **prefer the chat + right-pane composition** as the primary agent loop; keep Inventory/Visuals library as the vault.

### A. Chat + right Visual pane (preferred loop)

```text
┌──────── chat / activity ────────┬──────── Visual pane ────────┐
│ VISUALS rail icons              │ craftax.eval_matrix.v1      │
│ user / tool / thinking          │ cost vs performance chart   │
│ composer · Laguna XS 2.1        │ metric cards · achievements │
└─────────────────────────────────┴─────────────────────────────┘
```

This is the “def more of what we like” composition:
- Visuals appear as **chat-adjacent affordances** (rail / cards / activity cues)
- Opening a visual expands the **right pane** without leaving the conversation
- Template shells (`craftax.eval_matrix.v1`, annotation overlay, etc.) render through **one shared `VisualHost`**

### B. Inventory / Visuals vault + inspector

```text
Sidebar → Research/Visuals or Inventory → Visuals tab
  list of visual cards (title · templateId · timestamp · Open)
  + right inspector with the same VisualHost
```

Inventory “Visuals 8” list + Open → sealed-trace / overlay inspector is still valid as the **registry/library** view. The new first-class **Visuals** page (`data-testid="visuals-page"`) is the Rust-backed home for that vault. Do not invent a third identity — one `visual_id` across chat, pane, and library.

**IA rule**
- Conversations own the live show/hide loop
- Visuals library owns search, revisions, provenance, reopen
- Inventory keeps containers / traces / usage (visuals promoted out of being “just an inventory tab”)

---

## 2. Current ownership (after this branch of work)

```text
React (TS)
  ├── window.synthCore        → diagnostics + AppEvent replay/live
  ├── window.synthCodex       → Rust CodexManager
  ├── window.synthIntern      → Rust sync/async mailbox commands
  ├── window.synthProjects    → Rust project store
  ├── window.synthInventory   → Rust containers/traces/usage
  ├── window.synthVisuals     → Rust Visual Registry
  └── VisualHost / chat cards / VisualsPage / VisualPane

Rust CoreRuntime
  ├── storage/                SQLite + CAS + unified event journal
  ├── domain/                 shared SessionService / RunService / receipts
  ├── cloud/intern/           Rust SDK, pollers, ingestion, reconciliation
  ├── projects + inventory    durable project/container/trace/usage authority
  ├── visuals/ + visuals_ipc  registry, revisions, templates, MCP adapter
  ├── migration/              confirmed legacy import + rollback receipts
  ├── codex.rs                provider integration + MCP home wiring
  └── laguna.rs               MLX sidecar lifecycle only

Python (packaged sidecar only)
  └── services/laguna-daemon  Responses ↔ MLX

Retained, not packaged or auto-started
  └── services/local-runtime  migration/reference contracts + explicit legacy scripts
```

### Foundations landed
- Unified `AppEvent` / `VisualRecord` contracts in `@synth/runtime-protocol` + fixtures
- Rust SQLite schema (projects/sessions/runs/events/visuals/revisions/…)
- Codex notifications append to journal; `runtime:event` emit
- Visual Registry CRUD/save/fork/archive/show + CAS
- `synth-visuals-mcp` + Codex home MCP injection when binary present
- Shared `VisualHost`, Visuals page, Playwright visuals specs
- Bombadil prefers `dist/`; `testing.md` coverage map

These are now completed end-to-end Desktop slices. The browser-only development fixture still exposes an HTTP-shaped `synthRuntime` shim so Playwright can run without Tauri; the packaged Desktop does not install or call that bridge.

### Still Python

- Laguna/MLX Responses inference sidecar
- retired `services/local-runtime` source and explicit `legacy-*` scripts, retained only until migration/dogfood sign-off

---

## 3. Target architecture

```text
┌──────────────────────────── SYNTH DESKTOP ────────────────────────────┐
│ TS/React projections only                                             │
│   Chat · Visuals library · VisualHost pane · Cloud desk · Inventory   │
└───────────────────────────────┬───────────────────────────────────────┘
                                │ Tauri commands + runtime:event
┌───────────────────────────────▼───────────────────────────────────────┐
│ RUST CoreRuntime                                                      │
│  storage (SQLite + CAS) · event journal                               │
│  sessions/runs · visuals · traces · inventory                         │
│  CodexManager · InternClient (Rust SDK) · MlxManager                  │
└───────┬───────────────────┬───────────────────┬───────────────────────┘
        │                   │                   │
        ▼                   ▼                   ▼
 Codex app-server    Synth API              Laguna Responses
 (stdio JSON-RPC)    /smr/research-intern/*  :7333 → MLX sidecar
                     (same wire as synth-ai)
```

**Ownership invariant:** every durable object has one authority — Rust. MCP and React never open SQLite. Intern remote IDs stay metadata; local session/run/event sequences stay local.

---

## 4. Visuals — completion criteria & remaining polish

### Product behavior (must match screenshots)
1. Agent or user creates a visual → one `visual_id` in registry
2. Chat shows rail/card; Open expands right pane with `VisualHost`
3. Visuals library lists the same id; Open uses the same host
4. MCP `visual_show` / `visual_open_in_pane` emits real `visual.show` → UI opens pane (not `{opened:true}` stub)
5. Annotation overlay / eval matrix / rollout scrub templates render from `@synth/visuals` shells + bindings

### Code map
| Area | Path |
| --- | --- |
| Registry | `apps/synth_desktop/src-tauri/src/visuals/` |
| IPC | `src-tauri/src/visuals_ipc.rs` |
| MCP bin | `src-tauri/src/bin/synth_visuals_mcp.rs` |
| Host UI | `src/renderer/src/components/VisualHost.tsx` |
| Library | `src/renderer/src/components/VisualsPage.tsx` |
| Bridge | `src/renderer/src/runtime/desktopBridge.ts` (`synthVisuals`) |
| Tests | `tests/playwright/visuals-registry.spec.ts` |

### Remaining visuals work
- Commit visual mutation, revision, relationships, and `visual.*` journal event in one SQLite transaction
- Route all committed events through one `CoreRuntime` post-commit broadcaster
- Make visuals IPC publish MCP create/update/show events instead of only returning event JSON
- Subscribe React to `runtime:event`, replay from the journal, and project `visual.created` into the originating chat
- Replace the split `Record<string, unknown>` / `VisualBinding[]` / spread-props binding representations with one canonical contract
- Resolve trace / CAS / run / fixture / live sources through a shared resolver before invoking a template shell
- Execute registry search/status/session/template filtering in SQLite before `LIMIT`
- Replace the placeholder `VisualErrorBoundary` wrapper with a real React error boundary
- Read full IPC request bodies and secure the connection file with user-only permissions
- Point `runtimeClient` / DemoFixturesBar at Rust registry (stop Python `simulate-live` as authority)
- Seed/demo fixtures that recreate Inventory “Visuals 8” craftax sealed-trace set into Rust DB
- Bombadil specs for Visuals page + pane open
- End-to-end MCP dogfood: Codex → `visual_create` → chat card → pane
- Trust boundary: sandbox agent-authored HTML/TSX separately from bundled templates

### Visual infrastructure exit gate

All of the following must pass before production template expansion:

1. UI create, MCP create, update, save, fork, archive, and show use the same registry and event path.
2. Every visual mutation and journal event is atomic.
3. Chat, vault, and pane resolve one `visual_id` and revision.
4. Restart replay restores visual cards and current pane state without fixture reconstruction.
5. Missing or invalid bindings render explicit product states; they never silently display demo metrics.
6. Template failures are isolated by a real error boundary.
7. Search and filtering remain correct beyond the first result page.
8. Agent-authored executable content is sandboxed separately from trusted bundled templates.

### Shared template platform

`template.json` is the source of truth. Introduce a versioned manifest schema that generates or validates:

- Rust template catalog metadata
- TypeScript template metadata
- the Vite static shell importer map
- MCP-visible template descriptions
- fixture and slot-schema tests

Required manifest fields:

```text
id · version · title · genre · description · tags · accent
renderer kind · shell entry
slots[]: name · required · accepted binding kinds · schema
capabilities: live · revisions · export · compare · trace-aware
preview fixture · accessibility metadata
```

Canonical runtime boundary:

```ts
type VisualBinding = {
  slot: string;
  kind: "trace_v5" | "local_cas" | "run" | "live_sse" | "fixture";
  source: string;
  path?: string;
  schema?: string;
};

type ResolvedVisualData = Record<string, unknown>;

type TemplateShellProps = {
  visual: VisualRecord;
  data: ResolvedVisualData;
};
```

Fixtures are available only in the template development/preview harness. Production visuals with unresolved or invalid data show `loading`, `empty`, `unavailable`, or `invalid` states.

Shared presentation primitives should include:

- visual chrome, titles, status, provenance, revision, and export controls
- metric cards and metric strips
- accessible chart axes, scales, legends, tooltips, and semantic summaries
- distribution, line, scatter, heatmap, and sparkline primitives
- sortable/filterable result tables
- timeline and rollout scrubbers
- comparison selectors and baseline/candidate deltas
- loading, empty, error, partial, and stale-data states
- responsive pane/vault layouts

### Initial eval template family

Build generic templates on those primitives before adding more one-off Craftax implementations:

| Template | Purpose |
| --- | --- |
| `eval.overview.v1` | headline metrics, pass rate, cost, latency, score distribution |
| `eval.case_table.v1` | searchable case-level results, errors, filters, linked traces |
| `eval.model_compare.v1` | models/efforts across metrics, uncertainty, cost, regressions |
| `eval.failure_analysis.v1` | clustered failures, examples, scorer disagreement |
| `eval.rollout_inspector.v1` | trajectory, tools, rewards, observations, annotations |
| `eval.live_run.v1` | progress, throughput, partial aggregates, failures |
| `eval.regression.v1` | baseline/candidate deltas and statistically meaningful regressions |

Craftax templates become specializations of shared eval primitives. They must not remain the fallback for missing or unknown eval data.

---

## 5. Intern via Rust SDK (primary remaining backend slice)

### Goal
Replace Python `InternHttpClient` + `InternAdapter` with a **Rust Intern SDK** that is API-compatible with `synth-ai`’s Research Intern client — Desktop is a viewer of the cloud mailbox, never a second commander.

### Do not reinvent
Mirror the Python SDK / MCP surface, not the Desktop demo adapter:

| Plane | synth-ai / HTTP | Desktop Rust destination |
| --- | --- | --- |
| Sync create | `POST /smr/research-intern/sync-sessions` | `InternClient::create_sync` |
| Sync get | `GET .../sync-sessions/{id}` | `get_sync` |
| Sync send | `POST .../sync-sessions/{id}/commands` | `send_sync` / `command_sync` |
| Sync events | `GET .../runtimes/sync/{id}/events?after_sequence=` | `sync_events` + journal ingest |
| Sync controls | pause / resume / close / intervene / answer | `command_sync` kinds |
| Async ensure | `POST .../async/ensure` | `ensure_async` |
| Async get | `GET .../async` | `get_async` |
| Async send | `POST .../async/messages` | `send_async` |
| Async commands | `POST .../async/commands` | `command_async` (pause/resume/cancel/checkpoint/…) |
| Async events | `GET .../async/events?after_sequence=` | `async_events` + journal ingest |

Auth: `Authorization: Bearer {SYNTH_API_KEY}` against configured backend URL (`synth_config` profiles: prod/staging/local). Secrets never cross into React.

### Suggested Rust layout

```text
apps/synth_desktop/src-tauri/src/cloud/
  mod.rs
  client.rs          # reqwest bearer client, retries, errors
  paths.rs           # /smr/research-intern/... constants
  sync.rs            # sync session + commands + events page
  async_runtime.rs   # ensure/get/send/command/events
  normalize.rs       # remote Intern events → AppEvent
  poller.rs          # per-session tail tasks with backoff
  demo.rs            # optional demo mailbox (feature-flagged)
```

Composition:

```rust
pub struct CoreRuntime {
    // ...
    pub cloud: Arc<InternManager>, // owns client + pollers
}
```

`InternManager` and `CodexManager` must use shared domain services rather than writing session/run/event tables independently:

```text
CodexManager ──┐
               ├──▶ SessionService / RunService ──▶ SQLite + EventJournal
InternManager ─┘
```

The shared services own session creation/restoration, run lifecycle, command IDs and receipts, approvals, status transitions, cursor advancement, restart reconciliation, and post-commit publication.

### Normalization rules
- Preserve remote `sequence` as `remote_sequence`; allocate local journal `sequence` / `session_sequence`
- Dedupe on `(session_id, source=intern, remote_sequence)`
- Map `agent_message` → chat transcript projection (already partially handled in `sessionView.ts`)
- Map approvals / intervene / open judgment items into durable approval records
- Async leave-safe / phase / budget come from projection metadata, not hard-coded banners
- Worker Codex SSE join (if `SMR_WORKER_API_KEY`) stays Rust-side activity; mailbox remains authoritative

### Modes (unchanged product semantics)

| Mode | When | Behavior |
| --- | --- | --- |
| `remote` | API key set, demo off | Real mailbox HTTP via Rust SDK |
| `demo` | `SYNTH_INTERN_DEMO=1` | Local scripted events (port `demo.rs`) |
| `unconfigured` | no key | Fail closed; composer guidance |

### Exit gate for Intern port
1. Sync create → send → events → restart → resume cursor — no Python runtime
2. Async ensure → send → leave app → reopen → reconcile — no Python runtime
3. Pause/resume/cancel/checkpoint work through Rust commands
4. React Cloud desk unchanged except transport (`synthRuntime` → Tauri Intern commands or unified session API)
5. Contract fixtures shared with `packages/runtime-protocol/fixtures/intern-sync-events.json`

### Implementation notes vs synth-ai
- Promise wire compatibility for the **exact mailbox subset** Desktop uses today (`intern_client.py`), not full Research Intern SDK parity (datasets, factory receipts, meta-threads, …)
- Grow toward synth-ai parity only where Desktop UI needs it (intervene, answer_interaction, presence)
- Prefer typed serde structs generated/checked against protocol fixtures over `serde_json::Value` everywhere — keep `Value` only at the unknown-field boundary
- Reuse `reqwest` already in `Cargo.toml`; add retry/backoff; do not block Tauri async executor on long polls (spawn dedicated tasks)

### Poller and command requirements

- exactly one active poller per remote mailbox/session
- cancellation when a session closes or changes identity
- exponential backoff with jitter and deterministic test configuration
- fail-closed behavior for authentication errors; no infinite retry storm
- bounded pagination and payload sizes
- command idempotency keys and durable receipts
- remote dedupe on local session + source + remote sequence
- malformed remote-event quarantine with diagnostics
- resume after application sleep, network loss, and restart
- defined shutdown/drain behavior
- safe profile/API-key changes while pollers are active
- local journal remains authoritative for UI replay; remote mailbox remains authoritative for cloud facts

---

## 6. Phased execution sequence

### Phase A — Restore green and finish the active Rust inventory slice

- fix current Rust compilation failures
- finish containers, traces, usage, counts, and Tauri bridge integration
- keep all tests green throughout subsequent phases

**Exit gate:** `cargo test`, TypeScript typecheck, protocol tests, and relevant Playwright suites pass; Inventory no longer needs Python reads.

### Phase B — CoreRuntime convergence

- make domain mutation + journal append atomic
- establish one post-commit broadcaster
- route Codex, Tauri commands, visual IPC/MCP, and future cloud events through it
- project `runtime:event` replay/live events into React session state
- demote/remove parallel `codex:event` product-state projection
- persist Codex session/thread/run status in SQLite as the authority

**Exit gate:** restart replay produces the same session/chat state as the live process and no event is displayed before commit.

### Phase C — Visual end-to-end correctness

- complete MCP broadcast/show flow
- canonicalize bindings and resolved slot data
- correct filtering/pagination and IPC framing
- add real error and invalid-data states
- cut legacy visual reads/simulation over to `synthVisuals`
- complete chat + pane + vault dogfood

**Exit gate:** local Laguna and configured-provider Codex each create a visual through MCP that appears once in chat, opens in the pane, exists in the vault, and survives restart.

### Phase D — Template and eval platform

- versioned manifest and generated catalogs/importers
- shared resolver and template shell contract
- shared visual/eval presentation primitives
- development harness, schemas, accessibility checks, and screenshots
- implement initial generic eval template family

**Exit gate:** a new eval template can be added through one manifest + shell + schema/fixture package without editing independent Rust and TypeScript registries or copying chart infrastructure.

### Phase E — Shared session/run services

- centralize session, run, approval, cursor, command receipt, and reconciliation operations
- adapt Codex to those services
- define the provider interface Intern will use

**Exit gate:** Codex is a provider feeding shared domain services, not a separate persistence/event authority.

### Phase F — Rust Intern SDK and pollers

- implement the Desktop-used sync/async mailbox subset
- add typed normalization and durable cursor handling
- implement poller lifecycle, retry, command receipts, and reconciliation
- retain demo mode behind the same provider boundary

**Exit gate:** sync create/send/events/restart and async ensure/send/leave/reopen both work without Python product runtime state.

### Phase G — Cloud UI transport cutover

- move Cloud desk/composer/session commands from Python HTTP to Rust Tauri commands/shared session API
- confirm product behavior is unchanged except transport and durability

**Exit gate:** Intern Live and Background dogfood paths never call the Python local runtime.

### Phase H — Legacy data migration

- detect and back up Python runtime databases
- import projects, sessions, runs, events, cursors, containers, traces, visuals, and usage
- preserve IDs, timestamps, remote identities, and event ordering
- hash referenced assets into the Rust content store
- produce migration receipt with counts and warnings
- verify foreign keys and SQLite integrity
- make migration idempotent and retain rollback data

**Exit gate:** representative legacy fixtures migrate twice safely with stable counts/IDs and no duplicated events.

### Phase I — Remove Python product runtime

- remove `RuntimeManager` Python spawn/proxy behavior
- remove renderer/runtime-client dependency on legacy `runtime_request`
- remove Python local-runtime resources from packaging
- delete `services/local-runtime` only after parity and migration gates
- retain only Laguna/MLX Responses Python services

**Exit gate:** no Python local-runtime process, package resource, database writer, HTTP call, or UI fallback remains.

### Phase J — Expand the production template catalog

- add domain-specific eval, fine-tuning, optimizer, and deployment templates on shared primitives
- add publication/export only as a separately approved scope

Do not delete Python until Intern dogfood, migration, rollback, and packaging-removal gates pass.

---

## 7. Testing expectations

See `testing.md`. Minimum for this handoff:

**Rust**
- domain mutation + journal append rollback/atomicity tests
- single post-commit broadcast tests for Tauri commands and MCP IPC
- visual binding/slot validation and registry filtering/pagination tests
- IPC framing, payload limit, authentication, and connection-file permission tests
- Intern client unit tests against recorded HTTP fixtures (from synth-ai / local-runtime tests)
- Cursor dedupe + restart reconcile integration tests
- Visual registry CRUD/revision tests plus failure injection between write/event boundaries

**Template platform**
- validate every `template.json` against the versioned manifest schema
- verify generated Rust catalog and TypeScript importer IDs match exactly
- validate every fixture against its declared slot schema
- render loading/empty/invalid/unavailable states without fixture fallback
- accessibility and screenshot coverage at pane and vault widths

**Playwright**
- Visuals library + pane (landed)
- MCP-created visual appears in originating chat and opens from `visual.show`
- journal replay restores chat visual cards after reload
- Sync desk send + agent_message in transcript (add when Rust Intern lands)
- Async leave/reopen reconcile (add)

**Bombadil**
- Composer invariants (landed)
- Add Visuals page / pane non-overlap when library is primary

**Dogfood**
1. Local Laguna Codex creates visual via MCP → pane
2. Configured-provider Codex same
3. Intern Live remote events + linked visual
4. Intern Background survives restart
5. Legacy user data migrates with matching counts and stable IDs
6. No Python product runtime in process list for those paths

---

## 8. Security / config

- `SYNTH_API_KEY` / worker keys stay in Rust (`~/.synth-desktop` env + `synth_config`)
- Visuals IPC token in `visuals-ipc.json`; MCP gets `SYNTH_VISUALS_IPC_FILE` + `SYNTH_SESSION_ID`
- Never accept arbitrary visual write paths; CAS only
- Agent HTML/TSX untrusted; bundled templates trusted

---

## 9. Immediate next actions

1. Fully quit any pre-cutover Synth Desktop process and launch the newly built `.app`; a webview reload alone cannot add Tauri commands.  
2. Resolve the staging worker/execution gap: a configured Intern Live run must emit an `agent_message`, and Background must emit a checkpoint/result event after accepted instructions. Desktop create/send/control/poll/restart coverage is already complete.  
3. Re-run only the output assertions after the staging worker is available; retain the existing session IDs/cursors as evidence and verify no replay duplicates.  
4. Run MCP `visual_create` + `visual_show` from both Laguna Codex and a configured provider; confirm one `visual_id` appears in chat, pane, and vault after restart.  
5. After Intern dogfood sign-off, delete the retained `services/local-runtime`, legacy scripts/tests, and browser HTTP vocabulary that are no longer useful as migration contracts.  
6. Treat template expansion (evals, fine-tuning, optimizers, deployment) as the next product slice on the existing manifest/binding primitives.

---

## 10. File checklist for the Intern SDK slice

Create:
- `src-tauri/src/cloud/{mod,client,paths,sync,async_runtime,normalize,poller,demo}.rs`
- `packages/runtime-protocol` Intern command/event types if missing
- Playwright `intern-sync.spec.ts` / `intern-async.spec.ts` with stub then real harness
- HTTP fixtures under `packages/runtime-protocol/fixtures/intern-*.json`

Modify:
- `core_runtime.rs` — hold `InternManager`
- `lib.rs` — Tauri commands `intern_*` or fold into session API
- `runtime/desktopBridge.ts` / `runtime-client` — transport swap
- `adapters/intern.py` — delete after parity
- Codex/README/`testing.md` — update ownership lines

Reference while coding:
- `synth_ai/sdk/research/research_intern.py`
- MCP tool names: `intern_sync_*`, `intern_async_*` (user-synth-research server)
- `services/local-runtime/src/synth_local_runtime/intern_client.py` (Desktop’s current subset)
- `apps/synth_desktop/HANDOFF_INTERN_LOCAL_SLOT.md` (IA / leave-safe / slot ≠ mailbox)

---

## 11. Completion criteria (whole migration)

Code, packaged-app, migration, and live transport criteria below are implemented and green. The only open operational assertion is staging worker output, called out in section 9.

- Rust is sole product backend authority  
- Python local-runtime not started or packaged  
- Python remains only as MLX/Responses sidecar  
- Codex + Intern use shared session/run services and normalize into one durable journal  
- Every durable domain mutation and journal event commits atomically  
- React restores product state from `runtime:event` replay and follows the same live stream  
- Visuals: one `visual_id` and revision in chat, library, pane; MCP create/show is real  
- Visual bindings use one canonical contract and resolve through shared source infrastructure  
- Production visuals never silently substitute demo fixture metrics  
- New templates use a versioned manifest, generated catalog/import map, shared primitives, and a test harness  
- Initial generic eval template family is available  
- Intern Live + Background transport/control/restart dogfood without Python product runtime; worker-produced output still pending staging service availability  
- Rust inventory owns containers, traces, and usage  
- Legacy IDs, event ordering, relationships, and assets migrate with a verifiable receipt  
- No legacy Python HTTP transport, runtime spawn, packaged resource, or database writer remains  
- Restart / reconnect / migration / rollback tests green  
