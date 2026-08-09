# LoRA / adapter support — deferred after v0.1

Status: **not a current product capability.** The Desktop deliberately has no
LoRA or Finetunes UI, no fixture catalog, no selector state, and no
“Adapters · Not wired” placeholder. A user must never infer that an adapter is
selectable or loaded.

`ExecutionTarget.adapter` and the storage columns remain protocol/storage
compatibility fields. `null` means the base model; it is not a partial LoRA
implementation. Do not build UI around that field until the full path below is
available.

## What was removed

- Composer adapter groups and the Finetunes Settings section.
- Static `AVAILABLE_LORAS` / `LoraAdapter` fixtures and `selectedLoraId` state.
- Adapter counts, install buttons, and all “not wired” placeholder copy.
- Dormant LoRA-specific CSS affordances and the misleading adapter debt flag.

The base-model Laguna XS and configured remote targets remain unchanged.

## How to reintroduce support safely

Re-add LoRAs in this order; do not start with a picker.

1. Define an inventory contract.
   - Rust owns adapter identity, revision, digest, readiness, and resolved local path.
   - Expose list/get through explicit Tauri bridge commands.
   - Do not use static fixtures or port/directory guessing.

2. Implement runtime loading and rollback.
   - Add a Laguna daemon/Rust manager reload operation that accepts an inventory id,
     resolves it to a validated path, and starts MLX with `--adapter-path`.
   - Base selection must clear the path and reload base weights.
   - Failed reloads preserve the previous known-good state and surface an honest error.

3. Persist the selected adapter.
   - New local sessions store the inventory identity in `target.adapter`.
   - Codex start and restore must preserve it, and must ensure Laguna has loaded the
     requested adapter before a turn begins.
   - Decide explicitly whether changing adapters in a chat starts a new session or
     performs a reload; communicate that choice in the UI.

4. Add UI last.
   - Composer and Settings read from the same inventory source of truth.
   - Show only ready local adapters; hide remote rows until a provider has a real
     adapter mapping.
   - Install/training controls are absent unless they invoke real operations.

5. Add real acceptance coverage.
   - Rust: inventory id → path validation, load, clear, rollback, persistence.
   - Installed Tauri: choose ready adapter → health reports it → first local turn
     uses it → restart restores it.
   - Playwright: only the renderer projection; it cannot prove MLX loaded anything.

## Relevant future touchpoints

| Need | Likely location |
| --- | --- |
| Inventory and bridge | `src-tauri/src/inventory.rs`, `src/renderer/src/env.d.ts` |
| Laguna reload | `src-tauri/src/laguna.rs`, `services/laguna-daemon/laguna_daemon/manager.py` |
| Target/session wiring | `src/renderer/src/runtime/sessionView.ts`, `runtime/nativeCodex.ts` |
| Protocol/storage | `packages/runtime-protocol/src/index.ts`, Rust session/run storage |
| Product UI | `components/Composer.tsx`, `components/SettingsPage.tsx` |

Remote adapters, adapter training, and shipping adapter weights are follow-up
products, not v0.1 scope.
