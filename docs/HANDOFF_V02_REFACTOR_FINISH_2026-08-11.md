# Handoff: Workshop v0.2 architecture refactor — finish line

**Date:** 2026-08-11  
**Branch:** `josh/v02-architecture-refactor` (also fast-forwarded to `origin/dev`)  
**Tip:** `67d64dc` — `test(desktop): tolerate two Synth Cloud models under exhausted allowance`  
**Plan of record:** [`docs/V02_REFACTOR_NOTES_2026-08-11.md`](./V02_REFACTOR_NOTES_2026-08-11.md)  
**Current-state map:** [`docs/ARCHITECTURE_MAP_2026-08-11.md`](./ARCHITECTURE_MAP_2026-08-11.md)  
**Style/audit:** `jstack-dev/.jstack/style/synth_style.md` § WORKSHOP CONFORMANCE AUDIT  

This handoff is for an engineer picking up **remaining follow-ons**. Waves 0–7 and the closeout lanes are landed and tested on the tip above. Do not re-litigate the noun tree or re-run Waves 0–7 unless conform regresses.

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
