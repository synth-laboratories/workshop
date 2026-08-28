# LoRA / adapter support — Laguna Composer + catalog

Status: **This Mac Laguna adapters are a current product capability.** The
catalog is the inventory. Laguna Composer can load a Laguna-compatible LoRA
onto Laguna XS 2.1. Qwen Optimizers adapters stay on the catalog Chat
Completions / Responses buttons and never appear on the Composer picker.

`ExecutionTarget.adapter` stores the catalog identity (`sha256:…`). `null` is
the base Laguna XS weights. Bytes stay on disk until the user hits **Publish**.

## Inventory

Rust owns adapter identity, digest, readiness, and the durable local path
(`state_root/loras/{hex}/`). List/get/import/archive/patch/publish go through
the Optimizers catalog and Tauri/MCP. There is no fixture catalog.

## Runtime loading

`laguna_set_adapter` resolves a catalog id, refuses non-Laguna bases, and
POSTs `{"adapter_path": …}` to `/v1/synth/models/{model}/load` (or `null` for
the base model). The daemon is not restarted and there is no `--adapter-path`
flag: `NativeMlxBackend.set_adapter` records the new path and releases the
resident weights, so the next turn pays a cold load. Failed reloads keep the
previous known-good adapter and surface an error.
Changing the picker in a Laguna chat reloads in place; it does not start a
new conversation.

## Persistence

New local sessions store the catalog id in `target.adapter`. Codex start and
restore keep that field. Opening a local chat reloads Laguna to the stored
adapter before the next turn.

## UI

- Optimizers catalog: rename, notes, tags, and explicit **Publish** for This Mac
  rows. Chat Completions and Responses stay family-native, including streaming.
- Composer and landing: Laguna adapter chip next to the model picker when
  Laguna XS is selected. Only `this_mac` + `inference` + `ready` rows whose
  base model contains `laguna` or `poolside`.

## Relevant touchpoints

| Need | Location |
| --- | --- |
| Inventory | `src-tauri/src/optimizers/local_lora.rs` |
| Laguna reload | `src-tauri/src/laguna.rs`, `laguna_set_adapter` |
| Target/session wiring | `runtime/sessionView.ts`, `runtime/nativeCodex.ts` |
| Product UI | `LagunaAdapterPicker.tsx`, `OptimizersPage.tsx` |
