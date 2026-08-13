# Handoff: Workshop v0.2 architecture refactor — finish line

**Date:** 2026-08-11  
**Branch:** `josh/v02-architecture-refactor` (also fast-forwarded to `origin/dev`)  
**Tip:** `67d64dc` — `test(desktop): tolerate two Synth Cloud models under exhausted allowance`  
**Plan of record:** [`docs/V02_REFACTOR_NOTES_2026-08-11.md`](./V02_REFACTOR_NOTES_2026-08-11.md)  
**Current-state map:** [`docs/ARCHITECTURE_MAP_2026-08-11.md`](./ARCHITECTURE_MAP_2026-08-11.md)  
**Style/audit:** `jstack-dev/.jstack/style/synth_style.md` § WORKSHOP CONFORMANCE AUDIT  

This handoff is for an engineer picking up **remaining follow-ons**. Waves 0–7 and the closeout lanes are landed and tested on the tip above. Do not re-litigate the noun tree or re-run Waves 0–7 unless conform regresses.

---

## CUA READY (independent tester)

**First CUA pass: FAIL.** Blockers are claimed fixed in this tree; that claim is **not** Gate P. An independent rerun is still required. Full tester runbook: [`HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md`](./HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md).

| Claimed fixed | Retest |
| --- | --- |
| CMP-01 duplicate user bubbles | One bubble per submit |
| Migration 12 / `usage_ledger` | Isolated data roots; open candidate once before prod on any shared old DB |
| Bad ranked-bars analysis visual | No crash |

### Commit → install (clean tree required)

`npm run desktop:install` / `scripts/desktop.sh install` refuse a dirty worktree (`require_clean_worktree`; root `README.md`).

```bash
cd /Users/joshuapurtell/Documents/GitHub/workshop
git checkout josh/v02-architecture-refactor   # or dev after merge
# commit WIP first — git status --porcelain must be empty
git rev-parse HEAD

./scripts/desktop.sh conform
npm run desktop:check
(cd apps/synth_desktop/src-tauri && cargo test --lib)
NODE_PATH="$(pwd)/node_modules" node --test apps/synth_desktop/tests/*.test.mjs
(cd apps/synth_desktop && npx playwright test)

npm run desktop:install          # → /Applications/Synth Desktop.app
# Prod friends ZIP → /Applications/Synth Desktop PROD.app
```

**Isolate data roots** (mandatory): named instance `./scripts/desktop-instance.sh cua candidate` → `~/.synth-desktop/instances/v02/candidate/data`, **or** launch the installed candidate with `SYNTH_DESKTOP_DATA_ROOT` / `SYNTH_DESKTOP_CONFIG` / `SYNTH_CODEX_HOME` pointing at a private dir. Leave prod on the default canonical roots. Details + env list in the CUA handoff / [`HANDOFF_ISOLATED_DEV_INSTANCES.md`](../apps/synth_desktop/HANDOFF_ISOLATED_DEV_INSTANCES.md).

---

## 1. What you inherit (done)

### Locked product nouns (do not invent parallel names)

```text
Workspace → Identity{Device,Credential,Account}
         → Session{SessionKind: Codex|Intern, RuntimeTarget, Run, Event}
         → RuntimeTarget{Local,Remote,Cloud,Intern}
         → Data{Container,Trace,Projection,UsageRecord}   # was Inventory
         → Visual{Template,Instance,Binding}
         → Optimizer
         → Store (substrate only)
```

### Architecture already in tree

| Area | Where |
| --- | --- |
| SessionKind + migration 9 | `src-tauri/src/domain/session_kind.rs` |
| RuntimeTarget + migration 10 | `src-tauri/src/domain/runtime_target.rs` |
| One usage ledger + Data + migration 11 | `src-tauri/src/data.rs`, `storage/usage_records.rs` |
| Codex split | `src-tauri/src/session/codex/{proto,manager,event_pump,telemetry,home,tests}.rs` |
| Traits | `session/persistence.rs` (`SessionPersistence`), `session/codex/proto.rs` (`ProviderTransport`) |
| AppError | `src-tauri/src/error.rs` — commands return `Result<T, AppError>` |
| Limits / http / supervisor / loopback | `limits.rs`, `http.rs`, `services/`, `ipc/` |
| Contract consts + drift CI | `contract/{commands,events}.rs` ↔ `bridge/protocolConstants.ts` + `scripts/check-desktop-contract-drift.sh` |
| Single `runtime:event` + origin | `contract/events.rs`, emit in `session/codex/*`, listen in `desktopBridge.ts` |
| Renderer store | `stores/sessionStore.ts`, `stores/applyRuntimeEvent.ts` |
| Thin App | `App.tsx` (~268 lines) + `hooks/useAppController.ts` + `ComposerDock` |
| Specta scaffold | `contract/specta.rs`, `generated/protocol.ts` (seed only) |
| Bridge diet | no `window.synth*` outside bridge; import `bridges` / `invokeCommand` |

### Verified green at tip

```text
./scripts/desktop.sh conform          # all core counters 0; env.d.ts=70; drift OK
cargo test --lib                      # 292 passed (from apps/synth_desktop/src-tauri)
npm run desktop:check                 # tsc + cargo check
node --test apps/synth_desktop/tests/*.test.mjs   # 127 passed (needs esbuild on NODE_PATH)
cd apps/synth_desktop && npx playwright test      # 150/150 passed
```

Conform entrypoint: `./scripts/desktop.sh conform` (or `./scripts/conform-desktop.sh`).  
CI job definition parked at `scripts/ci/desktop-conform.yml` (OAuth lacked `workflow` scope to write `.github/workflows/` — copy when you have a token with `workflow`).

---

## 2. What is left (finish these)

Ordered by leverage. Each item should improve a conform count, delete dead code, or complete the generated boundary.

### P0 — Full tauri-specta cutover

**Goal:** Rust serde types remain source of truth; TS mirrors only via codegen.

1. Annotate remaining command DTOs with `specta::Type` (and `AppError` if required).
2. Register all ~120 commands in `collect_commands!` / `contract/specta.rs`.
3. Regenerate:  
   `cargo test -p synth-desktop --lib export_specta_protocol_bindings`  
   → `src/renderer/src/generated/protocol.ts`
4. Point `bridge/invoke.ts` at generated bindings; shrink/retire hand `COMMANDS` map only when sets match.
5. Cut over `generate_handler!` → `specta_builder.invoke_handler()` **only when the collected set is complete**.
6. Keep `scripts/check-desktop-contract-drift.sh` green the whole time.

**Done when:** `packages/runtime-protocol` is generated-or-retired; `bridge/types.ts` is not a second hand mirror; CI fails on drift.

### P1 — Delete Codex reconciliation helpers (Wave 1 leftover)

Still present in `session/codex/manager.rs` (and callers):

- `reconcile_failed_turn_start`
- `mark_detached_turn_interrupted`
- list-time / restart repair paths using `interrupt_active_run`

They already go through `SessionPersistence` / `SessionService::transition`. Prove each is redundant under the state machine, then delete + tighten tests in `session/codex/tests.rs`.

**Done when:** no dedicated “reconcile_*” helpers; `status_magic_codex` stays 0; Codex tests still pass.

### P2 — Finish shared infra adoption

- Move **eval_driver** onto `ipc/loopback_server.rs` (visuals_ipc already on it).
- Wire Laguna / Whisper / remaining IPC into `ManagedService` + supervisor drain (skeleton exists).
- Migrate remaining MCP bins to `ipc/mcp_stdio.rs` (containers bin already uses it).
- Sweep leftover timeout literals into `limits.rs` / `limits.ts`.

**Done when:** hand-rolled HTTP framing in eval_driver gone; quit path drains registered services.

### P3 — Product/API cleanup

- Retire or generate `packages/runtime-protocol` (see P0).
- `DELETE packages/runtime-client` if still 0 importers.
- Quarantine `apps/mock`, `apps/_ref_first_pass`, `services/local-runtime`, `services/local-inference` per architecture map §10.
- Decide `contracts/research-v1.json`: codegen or delete.
- Optional: rename remaining internal `inventoryOpen` / `onOpenInventory` / `window.synthInventory` transport names to Data (UI noun + `data_*` commands already landed; Playwright testids intentionally still `inventory-*`).

### P4 — Hygiene / ops

- Copy `scripts/ci/desktop-conform.yml` → `.github/workflows/` with a `workflow`-scoped token.
- Update SynthStyle WORKSHOP audit synonyms (`SessionTarget`→`SessionKind`, `ExecutionTarget`→`RuntimeTarget`, `Inventory`→`Data`) on next style-doc pass.
- Codex muse worktree at `.../Codex/.../workshop-muse-cua` may still show local `dev` at old SHA — `git fetch && git checkout dev && git pull` there; product tip is `origin/dev`.

---

## 3. How to work

```bash
git fetch origin
git checkout josh/v02-architecture-refactor   # or: git checkout dev && git pull
git pull

./scripts/desktop.sh conform
npm run desktop:check
(cd apps/synth_desktop/src-tauri && cargo test --lib)
NODE_PATH="$(pwd)/node_modules" node --test apps/synth_desktop/tests/*.test.mjs
(cd apps/synth_desktop && npx playwright test)
```

**PR protocol** (from SynthStyle updater-agent notes):

1. Name the rule ids closed (`prefer_hierarchies_of_clear_nouns`, `one_authoritative_source`, …).
2. Cite the paragon imitated (`domain/session_run.rs`, `preferences/`, `InferencePanel`, CredentialBroker `serve()`, …).
3. Paste conform before/after counts (must not increase).
4. Name objective test: smoke = `desktop.sh check` / `conform`; functional = wave “done when”.

**Do not:**

- Reintroduce `window.synth*` call sites, raw `invoke("...")`, `Client::new()`, or authority `OnceLock`s.
- Persist routing as opaque `target_json` kind strings — use `SessionKind` + `RuntimeTarget`.
- Grow `App.tsx` back into an orchestration god file — extend `useAppController` / stores / runtime modules.
- Invent a second usage ledger or resurrect `usage_ledger`.

---

## 4. Key files cheat sheet

```text
docs/V02_REFACTOR_NOTES_2026-08-11.md     plan + closeout table
docs/ARCHITECTURE_MAP_2026-08-11.md       pre-refactor topology
architecture.md                           Intern|Codex routing law

apps/synth_desktop/src-tauri/src/
  domain/{session_kind,runtime_target,session_run}.rs
  session/codex/*  session/persistence.rs
  contract/{commands,events,specta}.rs  error.rs  limits.rs  http.rs  data.rs
  ipc/  services/

apps/synth_desktop/src/renderer/src/
  App.tsx  hooks/useAppController.ts  stores/*  bridge/*  generated/protocol.ts
  runtime/desktopBridge.ts  components/DataPage.tsx

scripts/conform-desktop.sh
scripts/check-desktop-contract-drift.sh
scripts/ci/desktop-conform.yml
```

---

## 5. Suggested first PR

**Title:** `refactor(desktop): specta Type on RuntimeTarget/SessionKind + 10 highest-traffic commands`

Small, mergeable slice of P0: annotate core domain types, export a non-seed command set, regenerate `generated/protocol.ts`, keep dual-path handler, prove drift + `desktop:check` + Playwright smoke (`design-debt` design locks + `synth-cloud-provider`).

Then continue command batches until cutover is safe.

---

## 6. Finish-run update (2026-08-11)

This handoff was executed through the safe cutover boundary.

### Completed

- All 120 commands currently registered by the Desktop handler are annotated
  for Specta and collected by `contract/specta.rs`; their reachable DTO/result
  graph derives `specta::Type`.
- The Wave 1 Codex reconciliation helpers and list/restart repair paths were
  deleted. Process-exit finalization now belongs to the attached transport and
  completes before failed RPC waiters resume.
- EvalDriver now uses the shared Hyper loopback JSON server; its hand-written
  HTTP parser/framing is gone.
- All three MCP binaries use `ipc/mcp_stdio.rs`.
- Laguna and Whisper implement `ManagedService`, are registered with the
  supervisor, and are drained by the composition root on exit.
- The Desktop conform workflow is installed at
  `.github/workflows/desktop-conform.yml`.

### Specta cutover blocker

The collected boundary compiles, but the current exporter refuses existing
`i64`/`u64` command fields because JavaScript numbers cannot represent every
value without precision loss. Enabling
`Builder::dangerously_cast_bigints_to_number()` is not acceptable: besides the
precision loss, this dependency version recursively traverses
`serde_json::Value` and stack-overflows. The export test is therefore ignored
with the blocker in its annotation; `generate_handler!`, the committed seed
binding, and the const-based bridge remain authoritative until the DTO wire
types are made JS-safe or Specta gains a safe serializer mapping.

### Cleanup decisions

- `packages/runtime-client` is not a zero-importer package:
  `apps/_ref_first_pass/package.json` still depends on it, and the workspace
  lock links it. It was not deleted.
- The mock/reference apps and legacy Python services remain quarantined by
  architecture and product rules. They were not moved because the repository
  has no physical quarantine/exclusion convention and `apps/*` is still a root
  workspace glob.
- `contracts/research-v1.json` remains referenced by the handoff package and
  legacy documentation, so deleting it would be a separate contract-retirement
  decision rather than refactor cleanup.

### Verification

```text
./scripts/desktop.sh conform                         green (118 / 9 / 2 drift)
npm run desktop:check                               green
cargo test --lib                                    288 passed, 1 ignored
NODE_PATH="$(pwd)/node_modules" node --test ...     127 passed
cd apps/synth_desktop && npx playwright test        150 passed
```

---

## 7. CUA vs prod — blockers addressed in tree

See **CUA READY** above for the tester install sequence. Engineer detail:

| Blocker | Status |
| --- | --- |
| P1 duplicate user bubbles | **Fixed** — `clientMessageId` through `sendTurn`/`startTurn` → `record_user_prompt`. Tests: `user_message_ownership.test.mjs`, Rust `turn_send_reuses_client_message_id_in_journalled_user_prompt`, Playwright `session-lifecycle`. |
| Migration 11 dropped `usage_ledger` | **Fixed** — migration 11 keeps empty ledger as rollback buffer; migration 12 recreates it for already-dropped DBs; each open folds rollback writes once. Open candidate once before expecting prod to open the same profile. |
| Provenance drift | **Reconciled in docs** — `PROVENANCE.md` now records served ZIP `d31776…` / bundle `0.1.0`. Source SHA for those bytes still unbound; next friends cut must re-bind FE env + this file together. |
| Malformed analysis visual | **Hardened** — `analysis.visual.v1` `normalizeBlock` rejects blocks missing required arrays instead of `.map` on undefined. |

Receipt (FAIL): `/Users/joshuapurtell/Documents/Codex/2026-08-11/f/outputs/CUA_REFACTOR_VS_PROD_RECEIPT_2026-08-11.md`  
Runbook: [`HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md`](./HANDOFF_CUA_REFACTOR_VS_PROD_QA_2026-08-11.md)

**Still required before promote:** independent Tier A–C CUA with isolated data roots after the WIP tip is committed and installed.

### Implementation-session repair verification (2026-08-12)

- A fresh native candidate turn rendered one user bubble, streamed to completion,
  and returned to idle.
- The previous production app launched against the schema-12 profile and opened
  Inventory, including the Usage count, without `no such table: usage_ledger`.
- Automated verification: conform green; desktop check green; 292 Rust tests,
  129 renderer tests, and 152 Playwright tests passed.
